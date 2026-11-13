//! Multi-Core TCP Network Engine for VelocityDB using Glommio and `io_uring`.
//!
//! # Architecture & Pipelining:
//! - **Thread-per-Core**: Spawns an independent `LocalExecutor` pinned to each online CPU core
//!   discovered via `glommio::CpuSet`.
//! - **Zero Cross-Thread Locks**: Each core owns an isolated `LocalCache` (`Rc<RefCell<...>>`),
//!   completely eliminating mutexes and cache-line contention.
//! - **Kernel Connection Distribution (`SO_REUSEPORT`)**: Each core binds its own `TcpListener`
//!   to `0.0.0.0:6379`. The Linux kernel distributes connections across cores via `io_uring`.
//! - **Aggressive TCP Pipelining & Response Batching (3M+ RPS Architecture)**:
//!   When multiple commands (e.g. 50 pipelined requests) arrive in the same TCP buffer, all
//!   commands are parsed zero-copy and executed sequentially against `LocalCache`. Their responses
//!   are accumulated into a dedicated stack-allocated batch buffer and committed in a **single**
//!   socket write syscall/submission, drastically minimizing kernel context switches and SQE overhead.

use crate::aof::{self, AofEngine, AofSerializer};
use crate::db::LocalCache;
use crate::parser::{self, RespFrame};
use crate::pubsub::{self, LocalSender, PubSubHub, PubSubMessage};
use crate::replication::{self, PsyncResult, ReplicationHub};
use crate::security::{acl, AclEngine, ConnectionAcl};
#[cfg(target_os = "linux")]
use crate::security::AsyncTlsStream;
#[cfg(test)]
use crate::security::create_test_tls_config;
#[cfg(any(target_os = "linux", test))]
use crate::security::StreamWrapper;
use futures_lite::io::AsyncWriteExt;
#[cfg(target_os = "linux")]
use rustls::ServerConfig;
use std::rc::Rc;
#[cfg(target_os = "linux")]
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;

/// Global real-time throughput counter shared between io_uring data plane and management HTTP plane.
pub static GLOBAL_TOTAL_OPS: AtomicU64 = AtomicU64::new(0);

/// Global active TCP client connection counter.
pub static GLOBAL_ACTIVE_CONNS: AtomicUsize = AtomicUsize::new(0);

/// Default Redis port for VelocityDB.
pub const DEFAULT_PORT: u16 = 6379;

/// Default binding address.
pub const DEFAULT_BIND_ADDR: &str = "0.0.0.0:6379";

/// Stack buffer size for incoming TCP socket reads (16 KiB).
pub const STACK_BUFFER_SIZE: usize = 16384;

/// Stack buffer size for batching outgoing TCP socket writes (16 KiB).
pub const OUTPUT_BUFFER_SIZE: usize = 16384;

/// Static RESP byte response slices (zero heap allocation).
pub const RESP_OK: &[u8] = b"+OK\r\n";
pub const RESP_PONG: &[u8] = b"+PONG\r\n";
pub const RESP_NULL: &[u8] = b"$-1\r\n";
pub const RESP_ERR_UNKNOWN: &[u8] = b"-ERR unknown command\r\n";
pub const RESP_ERR_WRONG_ARGS_SET: &[u8] = b"-ERR wrong number of arguments for 'set' command\r\n";
pub const RESP_ERR_WRONG_ARGS_GET: &[u8] = b"-ERR wrong number of arguments for 'get' command\r\n";
pub const RESP_ERR_WRONG_ARGS_DEL: &[u8] = b"-ERR wrong number of arguments for 'del' command\r\n";
pub const RESP_ERR_WRONG_ARGS_SUBSCRIBE: &[u8] = b"-ERR wrong number of arguments for 'subscribe' command\r\n";
pub const RESP_ERR_WRONG_ARGS_PUBLISH: &[u8] = b"-ERR wrong number of arguments for 'publish' command\r\n";

/// Writes an ASCII unsigned integer into a byte buffer without heap allocations.
#[inline]
pub fn write_usize_to_buf(mut n: usize, buf: &mut [u8]) -> usize {
    if n == 0 {
        buf[0] = b'0';
        return 1;
    }
    let mut temp = [0u8; 20];
    let mut i = 0;
    while n > 0 {
        temp[i] = b'0' + (n % 10) as u8;
        n /= 10;
        i += 1;
    }
    let mut out_idx = 0;
    while i > 0 {
        i -= 1;
        buf[out_idx] = temp[i];
        out_idx += 1;
    }
    out_idx
}

/// Writes an ASCII signed 64-bit integer into a byte buffer without heap allocations.
#[inline]
pub fn write_i64_to_buf(n: i64, buf: &mut [u8]) -> usize {
    if n == 0 {
        buf[0] = b'0';
        return 1;
    }
    let is_neg = n < 0;
    let mut temp = [0u8; 20];
    let mut i = 0;

    let mut un = if is_neg {
        (n as i128).unsigned_abs() as u64
    } else {
        n as u64
    };

    while un > 0 {
        temp[i] = b'0' + (un % 10) as u8;
        un /= 10;
        i += 1;
    }

    let mut out_idx = 0;
    if is_neg {
        buf[out_idx] = b'-';
        out_idx += 1;
    }

    while i > 0 {
        i -= 1;
        buf[out_idx] = temp[i];
        out_idx += 1;
    }
    out_idx
}

/// Serializes a RESP bulk string into a stack buffer without heap allocations.
/// Returns total bytes written to `buf`, or `None` if `buf` is too small.
#[inline]
pub fn format_bulk_string_to_slice(val: &[u8], buf: &mut [u8]) -> Option<usize> {
    if buf.len() < 1 + 20 + 2 + val.len() + 2 {
        return None;
    }

    let mut offset = 0;
    buf[offset] = b'$';
    offset += 1;

    let len_digits = write_usize_to_buf(val.len(), &mut buf[offset..]);
    offset += len_digits;

    buf[offset] = b'\r';
    buf[offset + 1] = b'\n';
    offset += 2;

    buf[offset..offset + val.len()].copy_from_slice(val);
    offset += val.len();

    buf[offset] = b'\r';
    buf[offset + 1] = b'\n';
    offset += 2;

    Some(offset)
}

/// Serializes a RESP integer into a stack buffer without heap allocations.
#[inline]
pub fn format_integer_to_slice(n: i64, buf: &mut [u8]) -> usize {
    let mut offset = 0;
    buf[offset] = b':';
    offset += 1;

    let int_bytes = write_i64_to_buf(n, &mut buf[offset..]);
    offset += int_bytes;

    buf[offset] = b'\r';
    buf[offset + 1] = b'\n';
    offset += 2;

    offset
}

/// Zero-allocation, stack-buffered batch response writer.
///
/// Batches up to `OUTPUT_BUFFER_SIZE` bytes of serialized RESP responses directly
/// on the thread stack frame before committing a single write system call.
pub struct BatchWriter<'a, W> {
    writer: &'a mut W,
    buf: [u8; OUTPUT_BUFFER_SIZE],
    pos: usize,
}

impl<'a, W: AsyncWriteExt + Unpin> BatchWriter<'a, W> {
    /// Creates a new `BatchWriter` wrapping an underlying async writer.
    #[inline]
    pub fn new(writer: &'a mut W) -> Self {
        Self {
            writer,
            buf: [0u8; OUTPUT_BUFFER_SIZE],
            pos: 0,
        }
    }

    /// Appends a byte slice to the batch buffer.
    /// Flushes to the underlying socket if the buffer capacity is reached.
    pub async fn write_slice(&mut self, data: &[u8]) -> std::io::Result<()> {
        if self.pos + data.len() > OUTPUT_BUFFER_SIZE {
            self.flush_buffer().await?;
        }

        if data.len() > OUTPUT_BUFFER_SIZE {
            // Oversized payload exceeding full buffer capacity: write directly
            self.writer.write_all(data).await?;
        } else {
            self.buf[self.pos..self.pos + data.len()].copy_from_slice(data);
            self.pos += data.len();
        }

        Ok(())
    }

    /// Appends a RESP bulk string directly into the batch buffer without allocations.
    pub async fn write_bulk_string(&mut self, val: &[u8]) -> std::io::Result<()> {
        let mut header = [0u8; 32];
        header[0] = b'$';
        let len_digits = write_usize_to_buf(val.len(), &mut header[1..]);
        let header_len = 1 + len_digits + 2;
        header[1 + len_digits] = b'\r';
        header[1 + len_digits + 1] = b'\n';

        self.write_slice(&header[..header_len]).await?;
        self.write_slice(val).await?;
        self.write_slice(b"\r\n").await?;
        Ok(())
    }

    /// Appends a RESP integer directly into the batch buffer without allocations.
    pub async fn write_integer(&mut self, n: i64) -> std::io::Result<()> {
        let mut int_buf = [0u8; 32];
        let len = format_integer_to_slice(n, &mut int_buf);
        self.write_slice(&int_buf[..len]).await
    }

    /// Serializes and writes a RESP subscription notification:
    /// `*3\r\n$9\r\nsubscribe\r\n$<ch_len>\r\n<channel>\r\n:<count>\r\n`
    pub async fn write_pubsub_subscribe(&mut self, channel: &[u8], count: usize) -> std::io::Result<()> {
        self.write_slice(b"*3\r\n$9\r\nsubscribe\r\n").await?;
        self.write_bulk_string(channel).await?;
        self.write_integer(count as i64).await
    }

    /// Serializes and writes a RESP unsubscription notification:
    /// `*3\r\n$11\r\nunsubscribe\r\n$<ch_len>\r\n<channel>\r\n:<remaining_count>\r\n`
    pub async fn write_pubsub_unsubscribe(&mut self, channel: Option<&[u8]>, count: usize) -> std::io::Result<()> {
        self.write_slice(b"*3\r\n$11\r\nunsubscribe\r\n").await?;
        if let Some(ch) = channel {
            self.write_bulk_string(ch).await?;
        } else {
            self.write_slice(RESP_NULL).await?;
        }
        self.write_integer(count as i64).await
    }

    /// Serializes and writes a RESP pub/sub push message:
    /// `*3\r\n$7\r\nmessage\r\n$<ch_len>\r\n<channel>\r\n$<msg_len>\r\n<payload>\r\n`
    pub async fn write_pubsub_message(&mut self, channel: &[u8], payload: &[u8]) -> std::io::Result<()> {
        self.write_slice(b"*3\r\n$7\r\nmessage\r\n").await?;
        self.write_bulk_string(channel).await?;
        self.write_bulk_string(payload).await
    }

    /// Flushes currently buffered batch bytes to the socket.
    pub async fn flush_buffer(&mut self) -> std::io::Result<()> {
        if self.pos > 0 {
            self.writer.write_all(&self.buf[..self.pos]).await?;
            self.pos = 0;
        }
        Ok(())
    }

    /// Flushes all batched responses and commits the socket stream.
    pub async fn flush_all(&mut self) -> std::io::Result<()> {
        self.flush_buffer().await?;
        self.writer.flush().await?;
        Ok(())
    }

    /// Number of bytes currently pending in the un-flushed batch buffer.
    #[inline]
    pub fn pending_bytes(&self) -> usize {
        self.pos
    }
}

/// Transaction state maintained per client connection.
#[derive(Debug, Clone, Default)]
pub struct TransactionState {
    pub in_multi: bool,
    pub aborted: bool,
    pub cross_slot_error: bool,
    pub expected_slot: Option<u16>,
    pub queue: Vec<Vec<u8>>,
    pub asking: bool,
}

impl TransactionState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset(&mut self) {
        self.in_multi = false;
        self.aborted = false;
        self.cross_slot_error = false;
        self.expected_slot = None;
        self.queue.clear();
        self.asking = false;
    }
}

/// Extracts all key byte slices referenced by a RESP frame for hash slot verification.
pub fn extract_keys_from_frame(frame: &RespFrame<'_>) -> Vec<Vec<u8>> {
    let mut args = frame.clone().args();
    let cmd = match args.next() {
        Some(c) => c,
        None => return Vec::new(),
    };

    if cmd.eq_ignore_ascii_case(b"PING")
        || cmd.eq_ignore_ascii_case(b"AUTH")
        || cmd.eq_ignore_ascii_case(b"ACL")
        || cmd.eq_ignore_ascii_case(b"COMMAND")
        || cmd.eq_ignore_ascii_case(b"INFO")
        || cmd.eq_ignore_ascii_case(b"ROLE")
        || cmd.eq_ignore_ascii_case(b"MULTI")
        || cmd.eq_ignore_ascii_case(b"EXEC")
        || cmd.eq_ignore_ascii_case(b"DISCARD")
        || cmd.eq_ignore_ascii_case(b"CLUSTER")
        || cmd.eq_ignore_ascii_case(b"ASKING")
    {
        return Vec::new();
    }

    if cmd.eq_ignore_ascii_case(b"DEL") {
        return args.map(|k| k.to_vec()).collect();
    }

    if cmd.eq_ignore_ascii_case(b"XREAD") {
        let mut keys = Vec::new();
        let mut in_streams = false;
        let mut streams_args = Vec::new();
        while let Some(arg) = args.next() {
            if in_streams {
                streams_args.push(arg);
            } else if arg.eq_ignore_ascii_case(b"STREAMS") {
                in_streams = true;
            }
        }
        let half = streams_args.len() / 2;
        for &k in &streams_args[..half] {
            keys.push(k.to_vec());
        }
        return keys;
    }

    if let Some(key) = args.next() {
        vec![key.to_vec()]
    } else {
        Vec::new()
    }
}

/// Serializes a RESP frame into an owned RESP array byte buffer.
pub fn serialize_frame_to_bytes(frame: &RespFrame<'_>) -> Vec<u8> {
    let args: Vec<&[u8]> = frame.clone().args().collect();
    let mut out = Vec::new();
    out.push(b'*');
    out.extend_from_slice(args.len().to_string().as_bytes());
    out.extend_from_slice(b"\r\n");
    for arg in args {
        out.push(b'$');
        out.extend_from_slice(arg.len().to_string().as_bytes());
        out.extend_from_slice(b"\r\n");
        out.extend_from_slice(arg);
        out.extend_from_slice(b"\r\n");
    }
    out
}

/// Executes a parsed RESP frame with transaction support and Hash Tagging enforcement.
pub async fn execute_command_with_repl<W: AsyncWriteExt + Unpin>(
    frame: RespFrame<'_>,
    cache: &LocalCache,
    pubsub_hub: &PubSubHub,
    aof: &AofEngine,
    repl_hub: &ReplicationHub,
    acl_engine: &AclEngine,
    conn_acl: &mut ConnectionAcl,
    txn_state: &mut TransactionState,
    client_id: u64,
    tx: &LocalSender<PubSubMessage>,
    repl_tx: &LocalSender<Rc<[u8]>>,
    batch_writer: &mut BatchWriter<'_, W>,
) -> std::io::Result<()> {
    let mut args = frame.clone().args();
    let cmd = match args.next() {
        Some(c) => c,
        None => return Ok(()),
    };

    // 0. Cluster Mode & ASKING interception
    if cmd.eq_ignore_ascii_case(b"ASKING") {
        txn_state.asking = true;
        batch_writer.write_slice(RESP_OK).await?;
        return Ok(());
    }

    if cmd.eq_ignore_ascii_case(b"CLUSTER") {
        crate::cluster::handle_cluster_command(args, cache, batch_writer).await?;
        return Ok(());
    }

    let top = crate::cluster::get_topology();
    if top.enabled {
        let keys = extract_keys_from_frame(&frame);
        if let Some(primary_key) = keys.first() {
            let slot = crate::db::key_slot(primary_key);
            let key_exists = cache.contains_key(primary_key);
            let asking = txn_state.asking;
            txn_state.asking = false;

            match top.check_key_slot(slot, key_exists, asking) {
                crate::cluster::RedirectDecision::Local => {}
                crate::cluster::RedirectDecision::Moved { slot, target_addr } => {
                    let moved_resp = format!("-MOVED {slot} {target_addr}\r\n");
                    batch_writer.write_slice(moved_resp.as_bytes()).await?;
                    return Ok(());
                }
                crate::cluster::RedirectDecision::Ask { slot, target_addr } => {
                    let ask_resp = format!("-ASK {slot} {target_addr}\r\n");
                    batch_writer.write_slice(ask_resp.as_bytes()).await?;
                    return Ok(());
                }
                crate::cluster::RedirectDecision::ClusterDown => {
                    batch_writer.write_slice(b"-CLUSTERDOWN Hash slot not served\r\n").await?;
                    return Ok(());
                }
            }
        }
    }

    // 1. Transaction control commands
    if cmd.eq_ignore_ascii_case(b"MULTI") {
        if txn_state.in_multi {
            batch_writer.write_slice(b"-ERR MULTI calls can not be nested\r\n").await?;
        } else {
            txn_state.reset();
            txn_state.in_multi = true;
            batch_writer.write_slice(RESP_OK).await?;
        }
        return Ok(());
    }

    if cmd.eq_ignore_ascii_case(b"DISCARD") {
        if txn_state.in_multi {
            txn_state.reset();
            batch_writer.write_slice(RESP_OK).await?;
        } else {
            batch_writer.write_slice(b"-ERR DISCARD without MULTI\r\n").await?;
        }
        return Ok(());
    }

    if cmd.eq_ignore_ascii_case(b"EXEC") {
        if !txn_state.in_multi {
            batch_writer.write_slice(b"-ERR EXEC without MULTI\r\n").await?;
            return Ok(());
        }

        if txn_state.aborted {
            if txn_state.cross_slot_error {
                batch_writer.write_slice(b"-EXECABORT Transaction discarded because of cross-slot keys\r\n").await?;
            } else {
                batch_writer.write_slice(b"-EXECABORT Transaction discarded because of previous errors.\r\n").await?;
            }
            txn_state.reset();
            return Ok(());
        }

        let queued = std::mem::take(&mut txn_state.queue);
        txn_state.reset();

        batch_writer.write_slice(b"*").await?;
        batch_writer.write_slice(queued.len().to_string().as_bytes()).await?;
        batch_writer.write_slice(b"\r\n").await?;

        for cmd_bytes in &queued {
            if let Ok((queued_frame, _)) = parser::parse(cmd_bytes) {
                execute_command_internal(
                    queued_frame,
                    cache,
                    pubsub_hub,
                    aof,
                    repl_hub,
                    acl_engine,
                    conn_acl,
                    client_id,
                    tx,
                    repl_tx,
                    batch_writer,
                )
                .await?;
            }
        }
        return Ok(());
    }

    // 2. Hash Tagging & Shard Enforcement inside MULTI block
    if txn_state.in_multi {
        let keys = extract_keys_from_frame(&frame);
        let mut cross_slot = false;

        for key in &keys {
            let slot = crate::db::key_slot(key);
            match txn_state.expected_slot {
                Some(expected) => {
                    if expected != slot {
                        cross_slot = true;
                        break;
                    }
                }
                None => {
                    txn_state.expected_slot = Some(slot);
                }
            }
        }

        if cross_slot {
            txn_state.aborted = true;
            txn_state.cross_slot_error = true;
            batch_writer.write_slice(b"-CROSSSLOT Keys in MULTI block hash to different cores\r\n").await?;
            return Ok(());
        }

        let serialized = serialize_frame_to_bytes(&frame);
        txn_state.queue.push(serialized);
        batch_writer.write_slice(b"+QUEUED\r\n").await?;
        return Ok(());
    }

    // 3. Normal non-transaction execution
    execute_command_internal(
        frame,
        cache,
        pubsub_hub,
        aof,
        repl_hub,
        acl_engine,
        conn_acl,
        client_id,
        tx,
        repl_tx,
        batch_writer,
    )
    .await
}

/// Executes a single parsed RESP command against LocalCache, PubSub, AOF, and Replication.
pub async fn execute_command_internal<W: AsyncWriteExt + Unpin>(
    frame: RespFrame<'_>,
    cache: &LocalCache,
    pubsub_hub: &PubSubHub,
    aof: &AofEngine,
    repl_hub: &ReplicationHub,
    acl_engine: &AclEngine,
    conn_acl: &mut ConnectionAcl,
    client_id: u64,
    tx: &LocalSender<PubSubMessage>,
    repl_tx: &LocalSender<Rc<[u8]>>,
    batch_writer: &mut BatchWriter<'_, W>,
) -> std::io::Result<()> {
    let mut args = frame.args();
    let cmd = match args.next() {
        Some(c) => c,
        None => return Ok(()),
    };

    // Determine required permission category for O(1) authorization check
    let required_cat = if cmd.eq_ignore_ascii_case(b"PING") || cmd.eq_ignore_ascii_case(b"AUTH") {
        acl::CAT_CONNECTION
    } else if cmd.eq_ignore_ascii_case(b"GET")
        || cmd.eq_ignore_ascii_case(b"HGET")
        || cmd.eq_ignore_ascii_case(b"HEXISTS")
        || cmd.eq_ignore_ascii_case(b"HLEN")
        || cmd.eq_ignore_ascii_case(b"HGETALL")
        || cmd.eq_ignore_ascii_case(b"LLEN")
        || cmd.eq_ignore_ascii_case(b"LRANGE")
        || cmd.eq_ignore_ascii_case(b"LINDEX")
        || cmd.eq_ignore_ascii_case(b"ZSCORE")
        || cmd.eq_ignore_ascii_case(b"ZCARD")
        || cmd.eq_ignore_ascii_case(b"ZRANGE")
        || cmd.eq_ignore_ascii_case(b"ZREVRANGE")
        || cmd.eq_ignore_ascii_case(b"ZRANGEBYSCORE")
        || cmd.eq_ignore_ascii_case(b"TTL")
        || cmd.eq_ignore_ascii_case(b"GETBIT")
        || cmd.eq_ignore_ascii_case(b"BITCOUNT")
        || cmd.eq_ignore_ascii_case(b"XREAD")
        || cmd.eq_ignore_ascii_case(b"XLEN")
        || cmd.eq_ignore_ascii_case(b"XRANGE")
    {
        acl::CAT_READ
    } else if cmd.eq_ignore_ascii_case(b"SET")
        || cmd.eq_ignore_ascii_case(b"DEL")
        || cmd.eq_ignore_ascii_case(b"HSET")
        || cmd.eq_ignore_ascii_case(b"HDEL")
        || cmd.eq_ignore_ascii_case(b"LPUSH")
        || cmd.eq_ignore_ascii_case(b"RPUSH")
        || cmd.eq_ignore_ascii_case(b"LPOP")
        || cmd.eq_ignore_ascii_case(b"RPOP")
        || cmd.eq_ignore_ascii_case(b"ZADD")
        || cmd.eq_ignore_ascii_case(b"ZREM")
        || cmd.eq_ignore_ascii_case(b"EXPIRE")
        || cmd.eq_ignore_ascii_case(b"SETBIT")
        || cmd.eq_ignore_ascii_case(b"XADD")
    {
        acl::CAT_WRITE
    } else if cmd.eq_ignore_ascii_case(b"SUBSCRIBE")
        || cmd.eq_ignore_ascii_case(b"UNSUBSCRIBE")
        || cmd.eq_ignore_ascii_case(b"PUBLISH")
    {
        acl::CAT_PUBSUB
    } else if cmd.eq_ignore_ascii_case(b"PSYNC")
        || cmd.eq_ignore_ascii_case(b"SYNC")
        || cmd.eq_ignore_ascii_case(b"REPLCONF")
    {
        acl::CAT_REPLICATION
    } else if cmd.eq_ignore_ascii_case(b"ACL") {
        let mut peek_args = args.clone();
        if let Some(sub) = peek_args.next() {
            if sub.eq_ignore_ascii_case(b"WHOAMI") {
                acl::CAT_CONNECTION
            } else {
                acl::CAT_ADMIN
            }
        } else {
            acl::CAT_ADMIN
        }
    } else if cmd.eq_ignore_ascii_case(b"ROLE")
        || cmd.eq_ignore_ascii_case(b"INFO")
        || cmd.eq_ignore_ascii_case(b"COMMAND")
    {
        acl::CAT_CONNECTION
    } else {
        acl::CAT_CONNECTION
    };

    // Zero-allocation O(1) permission check (~0.5ns)
    if let Err(err_bytes) = conn_acl.check(required_cat) {
        batch_writer.write_slice(err_bytes).await?;
        return Ok(());
    }

    if cmd.eq_ignore_ascii_case(b"CLUSTER") {
        crate::cluster::handle_cluster_command(args, cache, batch_writer).await?;
        return Ok(());
    }

    if cmd.eq_ignore_ascii_case(b"ASKING") {
        batch_writer.write_slice(RESP_OK).await?;
        return Ok(());
    }

    if cmd.eq_ignore_ascii_case(b"PING") {
        if let Some(msg) = args.next() {
            batch_writer.write_bulk_string(msg).await?;
        } else {
            batch_writer.write_slice(RESP_PONG).await?;
        }
    } else if cmd.eq_ignore_ascii_case(b"SET") {
        let key = match args.next() {
            Some(k) => k,
            None => {
                batch_writer.write_slice(RESP_ERR_WRONG_ARGS_SET).await?;
                return Ok(());
            }
        };
        let val = match args.next() {
            Some(v) => v,
            None => {
                batch_writer.write_slice(RESP_ERR_WRONG_ARGS_SET).await?;
                return Ok(());
            }
        };

        // Support optional EX <seconds> or PX <milliseconds>
        let mut ttl: Option<Duration> = None;
        if let Some(opt) = args.next() {
            if opt.eq_ignore_ascii_case(b"EX") {
                if let Some(secs_bytes) = args.next() {
                    if let Ok(secs) = parser::parse_i64(secs_bytes) {
                        if secs > 0 {
                            ttl = Some(Duration::from_secs(secs as u64));
                        }
                    }
                }
            } else if opt.eq_ignore_ascii_case(b"PX") {
                if let Some(ms_bytes) = args.next() {
                    if let Ok(ms) = parser::parse_i64(ms_bytes) {
                        if ms > 0 {
                            ttl = Some(Duration::from_millis(ms as u64));
                        }
                    }
                }
            }
        }

        match ttl {
            Some(d) => cache.set_with_ttl(key, val, d),
            None => cache.set(key, val),
        }

        // Asynchronous non-blocking AOF persistence (~10ns) & Replication Backlog broadcast
        let record = AofSerializer::format_set(key, val, ttl);
        aof.append_raw(record.clone());
        repl_hub.record_and_broadcast(record);

        batch_writer.write_slice(RESP_OK).await?;
    } else if cmd.eq_ignore_ascii_case(b"GET") {
        let key = match args.next() {
            Some(k) => k,
            None => {
                batch_writer.write_slice(RESP_ERR_WRONG_ARGS_GET).await?;
                return Ok(());
            }
        };

        let mut stack_resp = [0u8; 4096];
        let stack_len = cache.with_value(key, |val| {
            format_bulk_string_to_slice(val, &mut stack_resp)
        });

        match stack_resp_result(stack_len) {
            StackResp::Formatted(len) => {
                batch_writer.write_slice(&stack_resp[..len]).await?;
            }
            StackResp::ValueTooLarge => {
                if let Some(val) = cache.get(key) {
                    batch_writer.write_bulk_string(&val).await?;
                } else {
                    batch_writer.write_slice(RESP_NULL).await?;
                }
            }
            StackResp::NotFound => {
                batch_writer.write_slice(RESP_NULL).await?;
            }
        }
    } else if cmd.eq_ignore_ascii_case(b"DEL") {
        let key = match args.next() {
            Some(k) => k,
            None => {
                batch_writer.write_slice(RESP_ERR_WRONG_ARGS_DEL).await?;
                return Ok(());
            }
        };

        let deleted = cache.del(key);
        if deleted {
            let record = AofSerializer::format_del(key);
            aof.append_raw(record.clone());
            repl_hub.record_and_broadcast(record);
        }
        batch_writer.write_integer(if deleted { 1 } else { 0 }).await?;
    } else if cmd.eq_ignore_ascii_case(b"SUBSCRIBE") {
        let mut has_channels = false;
        for ch in args {
            has_channels = true;
            let count = pubsub_hub.subscribe(ch, client_id, tx.clone());
            batch_writer.write_pubsub_subscribe(ch, count).await?;
        }
        if !has_channels {
            batch_writer.write_slice(RESP_ERR_WRONG_ARGS_SUBSCRIBE).await?;
        }
    } else if cmd.eq_ignore_ascii_case(b"UNSUBSCRIBE") {
        let mut has_channels = false;
        for ch in args {
            has_channels = true;
            let remaining = pubsub_hub.unsubscribe(ch, client_id);
            batch_writer.write_pubsub_unsubscribe(Some(ch), remaining).await?;
        }
        if !has_channels {
            let unsubs = pubsub_hub.unsubscribe_all(client_id);
            if unsubs.is_empty() {
                batch_writer.write_pubsub_unsubscribe(None, 0).await?;
            } else {
                for (ch, rem) in unsubs {
                    batch_writer.write_pubsub_unsubscribe(Some(&ch), rem).await?;
                }
            }
        }
    } else if cmd.eq_ignore_ascii_case(b"PUBLISH") {
        let channel = match args.next() {
            Some(ch) => ch,
            None => {
                batch_writer.write_slice(RESP_ERR_WRONG_ARGS_PUBLISH).await?;
                return Ok(());
            }
        };
        let message = match args.next() {
            Some(msg) => msg,
            None => {
                batch_writer.write_slice(RESP_ERR_WRONG_ARGS_PUBLISH).await?;
                return Ok(());
            }
        };
        let delivered = pubsub_hub.publish(channel, message);
        batch_writer.write_integer(delivered as i64).await?;
    } else if cmd.eq_ignore_ascii_case(b"HSET") {
        let key = match args.next() {
            Some(k) => k,
            None => {
                batch_writer.write_slice(b"-ERR wrong number of arguments for 'hset' command\r\n").await?;
                return Ok(());
            }
        };
        let field = match args.next() {
            Some(f) => f,
            None => {
                batch_writer.write_slice(b"-ERR wrong number of arguments for 'hset' command\r\n").await?;
                return Ok(());
            }
        };
        let val = match args.next() {
            Some(v) => v,
            None => {
                batch_writer.write_slice(b"-ERR wrong number of arguments for 'hset' command\r\n").await?;
                return Ok(());
            }
        };
        let is_new = cache.hset(key, field, val);
        let record = AofSerializer::format_hset(key, field, val);
        aof.append_raw(record.clone());
        repl_hub.record_and_broadcast(record);
        batch_writer.write_integer(if is_new { 1 } else { 0 }).await?;
    } else if cmd.eq_ignore_ascii_case(b"HGET") {
        let key = match args.next() {
            Some(k) => k,
            None => {
                batch_writer.write_slice(b"-ERR wrong number of arguments for 'hget' command\r\n").await?;
                return Ok(());
            }
        };
        let field = match args.next() {
            Some(f) => f,
            None => {
                batch_writer.write_slice(b"-ERR wrong number of arguments for 'hget' command\r\n").await?;
                return Ok(());
            }
        };
        if let Some(val) = cache.hget(key, field) {
            batch_writer.write_bulk_string(&val).await?;
        } else {
            batch_writer.write_slice(RESP_NULL).await?;
        }
    } else if cmd.eq_ignore_ascii_case(b"HDEL") {
        let key = match args.next() {
            Some(k) => k,
            None => {
                batch_writer.write_slice(b"-ERR wrong number of arguments for 'hdel' command\r\n").await?;
                return Ok(());
            }
        };
        let field = match args.next() {
            Some(f) => f,
            None => {
                batch_writer.write_slice(b"-ERR wrong number of arguments for 'hdel' command\r\n").await?;
                return Ok(());
            }
        };
        let del = cache.hdel(key, field);
        if del {
            let record = AofSerializer::format_hdel(key, field);
            aof.append_raw(record.clone());
            repl_hub.record_and_broadcast(record);
        }
        batch_writer.write_integer(if del { 1 } else { 0 }).await?;
    } else if cmd.eq_ignore_ascii_case(b"LPUSH") {
        let key = match args.next() {
            Some(k) => k,
            None => {
                batch_writer.write_slice(b"-ERR wrong number of arguments for 'lpush' command\r\n").await?;
                return Ok(());
            }
        };
        let val = match args.next() {
            Some(v) => v,
            None => {
                batch_writer.write_slice(b"-ERR wrong number of arguments for 'lpush' command\r\n").await?;
                return Ok(());
            }
        };
        let len = cache.lpush(key, val);
        let record = AofSerializer::format_lpush(key, val);
        aof.append_raw(record.clone());
        repl_hub.record_and_broadcast(record);
        batch_writer.write_integer(len as i64).await?;
    } else if cmd.eq_ignore_ascii_case(b"RPUSH") {
        let key = match args.next() {
            Some(k) => k,
            None => {
                batch_writer.write_slice(b"-ERR wrong number of arguments for 'rpush' command\r\n").await?;
                return Ok(());
            }
        };
        let val = match args.next() {
            Some(v) => v,
            None => {
                batch_writer.write_slice(b"-ERR wrong number of arguments for 'rpush' command\r\n").await?;
                return Ok(());
            }
        };
        let len = cache.rpush(key, val);
        let record = AofSerializer::format_rpush(key, val);
        aof.append_raw(record.clone());
        repl_hub.record_and_broadcast(record);
        batch_writer.write_integer(len as i64).await?;
    } else if cmd.eq_ignore_ascii_case(b"LPOP") {
        let key = match args.next() {
            Some(k) => k,
            None => {
                batch_writer.write_slice(b"-ERR wrong number of arguments for 'lpop' command\r\n").await?;
                return Ok(());
            }
        };
        if let Some(val) = cache.lpop(key) {
            let record = AofSerializer::format_lpop(key);
            aof.append_raw(record.clone());
            repl_hub.record_and_broadcast(record);
            batch_writer.write_bulk_string(&val).await?;
        } else {
            batch_writer.write_slice(RESP_NULL).await?;
        }
    } else if cmd.eq_ignore_ascii_case(b"RPOP") {
        let key = match args.next() {
            Some(k) => k,
            None => {
                batch_writer.write_slice(b"-ERR wrong number of arguments for 'rpop' command\r\n").await?;
                return Ok(());
            }
        };
        if let Some(val) = cache.rpop(key) {
            let record = AofSerializer::format_rpop(key);
            aof.append_raw(record.clone());
            repl_hub.record_and_broadcast(record);
            batch_writer.write_bulk_string(&val).await?;
        } else {
            batch_writer.write_slice(RESP_NULL).await?;
        }
    } else if cmd.eq_ignore_ascii_case(b"ZADD") {
        let key = match args.next() {
            Some(k) => k,
            None => {
                batch_writer.write_slice(b"-ERR wrong number of arguments for 'zadd' command\r\n").await?;
                return Ok(());
            }
        };
        let score_bytes = match args.next() {
            Some(s) => s,
            None => {
                batch_writer.write_slice(b"-ERR wrong number of arguments for 'zadd' command\r\n").await?;
                return Ok(());
            }
        };
        let score: f64 = match std::str::from_utf8(score_bytes).ok().and_then(|s| s.parse().ok()) {
            Some(sc) => sc,
            None => {
                batch_writer.write_slice(b"-ERR value is not a valid float\r\n").await?;
                return Ok(());
            }
        };
        let member = match args.next() {
            Some(m) => m,
            None => {
                batch_writer.write_slice(b"-ERR wrong number of arguments for 'zadd' command\r\n").await?;
                return Ok(());
            }
        };
        let is_new = cache.zadd(key, score, member);
        let record = AofSerializer::format_zadd(key, score, member);
        aof.append_raw(record.clone());
        repl_hub.record_and_broadcast(record);
        batch_writer.write_integer(if is_new { 1 } else { 0 }).await?;
    } else if cmd.eq_ignore_ascii_case(b"ZSCORE") {
        let key = match args.next() {
            Some(k) => k,
            None => {
                batch_writer.write_slice(b"-ERR wrong number of arguments for 'zscore' command\r\n").await?;
                return Ok(());
            }
        };
        let member = match args.next() {
            Some(m) => m,
            None => {
                batch_writer.write_slice(b"-ERR wrong number of arguments for 'zscore' command\r\n").await?;
                return Ok(());
            }
        };
        if let Some(score) = cache.zscore(key, member) {
            let score_str = score.to_string();
            batch_writer.write_bulk_string(score_str.as_bytes()).await?;
        } else {
            batch_writer.write_slice(RESP_NULL).await?;
        }
    } else if cmd.eq_ignore_ascii_case(b"ZREM") {
        let key = match args.next() {
            Some(k) => k,
            None => {
                batch_writer.write_slice(b"-ERR wrong number of arguments for 'zrem' command\r\n").await?;
                return Ok(());
            }
        };
        let member = match args.next() {
            Some(m) => m,
            None => {
                batch_writer.write_slice(b"-ERR wrong number of arguments for 'zrem' command\r\n").await?;
                return Ok(());
            }
        };
        let rem = cache.zrem(key, member);
        if rem {
            let record = AofSerializer::format_zrem(key, member);
            aof.append_raw(record.clone());
            repl_hub.record_and_broadcast(record);
        }
        batch_writer.write_integer(if rem { 1 } else { 0 }).await?;
    } else if cmd.eq_ignore_ascii_case(b"PSYNC") {
        if !repl_hub.is_replica(client_id) {
            repl_hub.register_replica(client_id, repl_tx.clone());
        }
        let req_replid = args.next().unwrap_or(b"?");
        let req_offset_str = args.next().unwrap_or(b"-1");
        let req_offset = parser::parse_i64(req_offset_str).unwrap_or(-1);
        match repl_hub.handle_psync(client_id, req_replid, req_offset, cache) {
            PsyncResult::Continue { replid, backlog_bytes } => {
                let mut header = Vec::with_capacity(64);
                header.extend_from_slice(b"+CONTINUE ");
                header.extend_from_slice(replid.as_bytes());
                header.extend_from_slice(b"\r\n");
                batch_writer.write_slice(&header).await?;
                if !backlog_bytes.is_empty() {
                    batch_writer.write_slice(&backlog_bytes).await?;
                }
            }
            PsyncResult::FullResync { replid, offset, snapshot_payload } => {
                let mut header = Vec::with_capacity(64);
                header.extend_from_slice(b"+FULLRESYNC ");
                header.extend_from_slice(replid.as_bytes());
                header.extend_from_slice(b" ");
                header.extend_from_slice(offset.to_string().as_bytes());
                header.extend_from_slice(b"\r\n");
                batch_writer.write_slice(&header).await?;
                batch_writer.write_bulk_string(&snapshot_payload).await?;
            }
        }
    } else if cmd.eq_ignore_ascii_case(b"SYNC") {
        if !repl_hub.is_replica(client_id) {
            repl_hub.register_replica(client_id, repl_tx.clone());
        }
        if let PsyncResult::FullResync { replid, offset, snapshot_payload } = repl_hub.handle_sync(client_id, cache) {
            let mut header = Vec::with_capacity(64);
            header.extend_from_slice(b"+FULLRESYNC ");
            header.extend_from_slice(replid.as_bytes());
            header.extend_from_slice(b" ");
            header.extend_from_slice(offset.to_string().as_bytes());
            header.extend_from_slice(b"\r\n");
            batch_writer.write_slice(&header).await?;
            batch_writer.write_bulk_string(&snapshot_payload).await?;
        }
    } else if cmd.eq_ignore_ascii_case(b"REPLCONF") {
        if !repl_hub.is_replica(client_id) {
            repl_hub.register_replica(client_id, repl_tx.clone());
        }
        let subcmd = args.next().unwrap_or(b"");
        if subcmd.eq_ignore_ascii_case(b"listening-port") {
            if let Some(port_bytes) = args.next() {
                if let Ok(port) = parser::parse_i64(port_bytes) {
                    repl_hub.set_replica_listening_port(client_id, port as u16);
                }
            }
            batch_writer.write_slice(RESP_OK).await?;
        } else if subcmd.eq_ignore_ascii_case(b"capa") {
            batch_writer.write_slice(RESP_OK).await?;
        } else if subcmd.eq_ignore_ascii_case(b"ACK") {
            if let Some(offset_bytes) = args.next() {
                if let Ok(off) = parser::parse_i64(offset_bytes) {
                    repl_hub.update_replica_ack(client_id, off);
                }
            }
            // ACK commands in Redis replication are silent
        } else if subcmd.eq_ignore_ascii_case(b"GETACK") {
            let offset = repl_hub.master_repl_offset();
            let offset_str = offset.to_string();
            let mut resp = Vec::with_capacity(64);
            resp.extend_from_slice(b"*3\r\n$8\r\nREPLCONF\r\n$3\r\nACK\r\n$");
            resp.extend_from_slice(offset_str.len().to_string().as_bytes());
            resp.extend_from_slice(b"\r\n");
            resp.extend_from_slice(offset_str.as_bytes());
            resp.extend_from_slice(b"\r\n");
            batch_writer.write_slice(&resp).await?;
        } else {
            batch_writer.write_slice(RESP_OK).await?;
        }
    } else if cmd.eq_ignore_ascii_case(b"ROLE") {
        let role_bytes = repl_hub.generate_role_resp();
        batch_writer.write_slice(&role_bytes).await?;
    } else if cmd.eq_ignore_ascii_case(b"INFO") {
        let section = args.next();
        if section.is_none() || section.unwrap().eq_ignore_ascii_case(b"replication") {
            let info_str = repl_hub.generate_info_replication();
            batch_writer.write_bulk_string(info_str.as_bytes()).await?;
        } else {
            batch_writer.write_bulk_string(b"# Info\r\n").await?;
        }
    } else if cmd.eq_ignore_ascii_case(b"AUTH") {
        let first = match args.next() {
            Some(a) => a,
            None => {
                batch_writer.write_slice(b"-ERR wrong number of arguments for 'auth' command\r\n").await?;
                return Ok(());
            }
        };
        let second = args.next();
        let (user, pass) = match second {
            Some(p) => (std::str::from_utf8(first).unwrap_or(""), p),
            None => ("default", first),
        };

        if let Some(new_acl) = acl_engine.authenticate(user, pass) {
            *conn_acl = new_acl;
            batch_writer.write_slice(RESP_OK).await?;
        } else {
            batch_writer.write_slice(b"-WRONGPASS invalid username-password pair or user is disabled.\r\n").await?;
        }
    } else if cmd.eq_ignore_ascii_case(b"ACL") {
        let sub = args.next().unwrap_or(b"");
        if sub.eq_ignore_ascii_case(b"WHOAMI") {
            batch_writer.write_bulk_string(conn_acl.username.as_bytes()).await?;
        } else if sub.eq_ignore_ascii_case(b"LIST") {
            let list_bytes = acl_engine.format_acl_list();
            batch_writer.write_slice(&list_bytes).await?;
        } else if sub.eq_ignore_ascii_case(b"USERS") {
            let usernames = acl_engine.list_usernames();
            let mut out = Vec::new();
            out.extend_from_slice(b"*");
            out.extend_from_slice(usernames.len().to_string().as_bytes());
            out.extend_from_slice(b"\r\n");
            for u in usernames {
                out.extend_from_slice(b"$");
                out.extend_from_slice(u.len().to_string().as_bytes());
                out.extend_from_slice(b"\r\n");
                out.extend_from_slice(u.as_bytes());
                out.extend_from_slice(b"\r\n");
            }
            batch_writer.write_slice(&out).await?;
        } else if sub.eq_ignore_ascii_case(b"SETUSER") {
            let username = match args.next() {
                Some(u) => std::str::from_utf8(u).unwrap_or(""),
                None => {
                    batch_writer.write_slice(b"-ERR wrong number of arguments for 'acl setuser'\r\n").await?;
                    return Ok(());
                }
            };
            let mut pass = None;
            let mut perms = acl::CAT_READ | acl::CAT_CONNECTION;
            for rule in args {
                if rule.starts_with(b">") {
                    pass = std::str::from_utf8(&rule[1..]).ok();
                } else if rule.eq_ignore_ascii_case(b"+@all") {
                    perms = acl::ALL_PERMISSIONS;
                } else if rule.eq_ignore_ascii_case(b"+@write") {
                    perms |= acl::CAT_WRITE;
                } else if rule.eq_ignore_ascii_case(b"+@read") {
                    perms |= acl::CAT_READ;
                }
            }
            acl_engine.set_user(username, pass, perms);
            batch_writer.write_slice(RESP_OK).await?;
        } else if sub.eq_ignore_ascii_case(b"DELUSER") {
            let username = match args.next() {
                Some(u) => std::str::from_utf8(u).unwrap_or(""),
                None => {
                    batch_writer.write_slice(b"-ERR wrong number of arguments for 'acl deluser'\r\n").await?;
                    return Ok(());
                }
            };
            let del = acl_engine.del_user(username);
            batch_writer.write_integer(if del { 1 } else { 0 }).await?;
        } else {
            batch_writer.write_slice(RESP_ERR_UNKNOWN).await?;
        }
    } else if cmd.eq_ignore_ascii_case(b"SETBIT") {
        let key = match args.next() {
            Some(k) => k,
            None => {
                batch_writer.write_slice(b"-ERR wrong number of arguments for 'setbit' command\r\n").await?;
                return Ok(());
            }
        };
        let off_b = match args.next() {
            Some(o) => o,
            None => {
                batch_writer.write_slice(b"-ERR wrong number of arguments for 'setbit' command\r\n").await?;
                return Ok(());
            }
        };
        let val_b = match args.next() {
            Some(v) => v,
            None => {
                batch_writer.write_slice(b"-ERR wrong number of arguments for 'setbit' command\r\n").await?;
                return Ok(());
            }
        };
        let offset = match parser::parse_i64(off_b) {
            Ok(o) if o >= 0 => o as usize,
            _ => {
                batch_writer.write_slice(b"-ERR bit offset is not an integer or out of range\r\n").await?;
                return Ok(());
            }
        };
        let val = match parser::parse_i64(val_b) {
            Ok(v) if v == 0 || v == 1 => v as u8,
            _ => {
                batch_writer.write_slice(b"-ERR bit is not an integer or out of range\r\n").await?;
                return Ok(());
            }
        };
        match cache.setbit(key, offset, val) {
            Ok(old_bit) => {
                let record = AofSerializer::format_setbit(key, offset, val);
                aof.append_raw(record.clone());
                repl_hub.record_and_broadcast(record);
                batch_writer.write_integer(old_bit as i64).await?;
            }
            Err(e) => {
                batch_writer.write_slice(b"-").await?;
                batch_writer.write_slice(e.as_bytes()).await?;
                batch_writer.write_slice(b"\r\n").await?;
            }
        }
    } else if cmd.eq_ignore_ascii_case(b"GETBIT") {
        let key = match args.next() {
            Some(k) => k,
            None => {
                batch_writer.write_slice(b"-ERR wrong number of arguments for 'getbit' command\r\n").await?;
                return Ok(());
            }
        };
        let off_b = match args.next() {
            Some(o) => o,
            None => {
                batch_writer.write_slice(b"-ERR wrong number of arguments for 'getbit' command\r\n").await?;
                return Ok(());
            }
        };
        let offset = match parser::parse_i64(off_b) {
            Ok(o) if o >= 0 => o as usize,
            _ => {
                batch_writer.write_slice(b"-ERR bit offset is not an integer or out of range\r\n").await?;
                return Ok(());
            }
        };
        let bit = cache.getbit(key, offset);
        batch_writer.write_integer(bit as i64).await?;
    } else if cmd.eq_ignore_ascii_case(b"BITCOUNT") {
        let key = match args.next() {
            Some(k) => k,
            None => {
                batch_writer.write_slice(b"-ERR wrong number of arguments for 'bitcount' command\r\n").await?;
                return Ok(());
            }
        };
        let start = args.next().and_then(|b| parser::parse_i64(b).ok()).map(|v| v as isize);
        let end = args.next().and_then(|b| parser::parse_i64(b).ok()).map(|v| v as isize);
        let count = cache.bitcount(key, start, end);
        batch_writer.write_integer(count as i64).await?;
    } else if cmd.eq_ignore_ascii_case(b"XADD") {
        let key = match args.next() {
            Some(k) => k,
            None => {
                batch_writer.write_slice(b"-ERR wrong number of arguments for 'xadd' command\r\n").await?;
                return Ok(());
            }
        };
        let id_or_field = match args.next() {
            Some(i) => i,
            None => {
                batch_writer.write_slice(b"-ERR wrong number of arguments for 'xadd' command\r\n").await?;
                return Ok(());
            }
        };
        let (id_req, first_field) = if id_or_field == b"*" || id_or_field.contains(&b'-') {
            (crate::db::StreamId::parse(id_or_field), args.next())
        } else {
            (None, Some(id_or_field))
        };

        let mut fields = Vec::new();
        if let Some(f1) = first_field {
            let v1 = match args.next() {
                Some(v) => v,
                None => {
                    batch_writer.write_slice(b"-ERR wrong number of arguments for 'xadd' command\r\n").await?;
                    return Ok(());
                }
            };
            fields.push((f1.to_vec(), v1.to_vec()));
            while let Some(f) = args.next() {
                let v = match args.next() {
                    Some(val) => val,
                    None => {
                        batch_writer.write_slice(b"-ERR wrong number of arguments for 'xadd' command\r\n").await?;
                        return Ok(());
                    }
                };
                fields.push((f.to_vec(), v.to_vec()));
            }
        } else {
            batch_writer.write_slice(b"-ERR wrong number of arguments for 'xadd' command\r\n").await?;
            return Ok(());
        }

        match cache.xadd(key, id_req, fields.clone()) {
            Ok(final_id) => {
                let final_id_str = final_id.format();
                let record = AofSerializer::format_xadd(key, &final_id_str, &fields);
                aof.append_raw(record.clone());
                repl_hub.record_and_broadcast(record);
                batch_writer.write_bulk_string(final_id_str.as_bytes()).await?;
            }
            Err(e) => {
                batch_writer.write_slice(b"-").await?;
                batch_writer.write_slice(e.as_bytes()).await?;
                batch_writer.write_slice(b"\r\n").await?;
            }
        }
    } else if cmd.eq_ignore_ascii_case(b"XREAD") {
        let mut count = None;
        let mut in_streams = false;
        let mut raw_stream_args = Vec::new();

        while let Some(arg) = args.next() {
            if in_streams {
                raw_stream_args.push(arg);
            } else if arg.eq_ignore_ascii_case(b"COUNT") {
                if let Some(c_b) = args.next() {
                    if let Ok(c) = parser::parse_i64(c_b) {
                        count = Some(c as usize);
                    }
                }
            } else if arg.eq_ignore_ascii_case(b"BLOCK") {
                let _ = args.next();
            } else if arg.eq_ignore_ascii_case(b"STREAMS") {
                in_streams = true;
            }
        }

        if raw_stream_args.is_empty() || raw_stream_args.len() % 2 != 0 {
            batch_writer.write_slice(b"-ERR wrong number of arguments for 'xread' command\r\n").await?;
            return Ok(());
        }

        let half = raw_stream_args.len() / 2;
        let mut streams = Vec::new();
        for i in 0..half {
            let key = raw_stream_args[i];
            let id_raw = raw_stream_args[half + i];
            let start_id = crate::db::StreamId::parse(id_raw).unwrap_or(crate::db::StreamId::new(0, 0));
            streams.push((key, start_id));
        }

        let results = cache.xread(&streams, count);
        if results.is_empty() {
            batch_writer.write_slice(RESP_NULL).await?;
        } else {
            batch_writer.write_slice(b"*").await?;
            batch_writer.write_slice(results.len().to_string().as_bytes()).await?;
            batch_writer.write_slice(b"\r\n").await?;

            for (stream_key, entries) in results {
                batch_writer.write_slice(b"*2\r\n").await?;
                batch_writer.write_bulk_string(&stream_key).await?;

                batch_writer.write_slice(b"*").await?;
                batch_writer.write_slice(entries.len().to_string().as_bytes()).await?;
                batch_writer.write_slice(b"\r\n").await?;

                for entry in entries {
                    batch_writer.write_slice(b"*2\r\n").await?;
                    let id_str = entry.id.format();
                    batch_writer.write_bulk_string(id_str.as_bytes()).await?;

                    let fields_count = entry.fields.len() * 2;
                    batch_writer.write_slice(b"*").await?;
                    batch_writer.write_slice(fields_count.to_string().as_bytes()).await?;
                    batch_writer.write_slice(b"\r\n").await?;

                    for (f, v) in entry.fields {
                        batch_writer.write_bulk_string(&f).await?;
                        batch_writer.write_bulk_string(&v).await?;
                    }
                }
            }
        }
    } else if cmd.eq_ignore_ascii_case(b"XLEN") {
        let key = match args.next() {
            Some(k) => k,
            None => {
                batch_writer.write_slice(b"-ERR wrong number of arguments for 'xlen' command\r\n").await?;
                return Ok(());
            }
        };
        let len = cache.xlen(key);
        batch_writer.write_integer(len as i64).await?;
    } else if cmd.eq_ignore_ascii_case(b"XRANGE") {
        let key = match args.next() {
            Some(k) => k,
            None => {
                batch_writer.write_slice(b"-ERR wrong number of arguments for 'xrange' command\r\n").await?;
                return Ok(());
            }
        };
        let start_raw = match args.next() {
            Some(s) => s,
            None => {
                batch_writer.write_slice(b"-ERR wrong number of arguments for 'xrange' command\r\n").await?;
                return Ok(());
            }
        };
        let end_raw = match args.next() {
            Some(e) => e,
            None => {
                batch_writer.write_slice(b"-ERR wrong number of arguments for 'xrange' command\r\n").await?;
                return Ok(());
            }
        };
        let start_id = if start_raw == b"-" {
            crate::db::StreamId::new(0, 0)
        } else {
            crate::db::StreamId::parse(start_raw).unwrap_or(crate::db::StreamId::new(0, 0))
        };
        let end_id = if end_raw == b"+" {
            crate::db::StreamId::new(u64::MAX, u64::MAX)
        } else {
            crate::db::StreamId::parse(end_raw).unwrap_or(crate::db::StreamId::new(u64::MAX, u64::MAX))
        };
        let mut count = None;
        if let Some(opt) = args.next() {
            if opt.eq_ignore_ascii_case(b"COUNT") {
                if let Some(c_b) = args.next() {
                    if let Ok(c) = parser::parse_i64(c_b) {
                        count = Some(c as usize);
                    }
                }
            }
        }
        let entries = cache.xrange(key, start_id, end_id, count);
        batch_writer.write_slice(b"*").await?;
        batch_writer.write_slice(entries.len().to_string().as_bytes()).await?;
        batch_writer.write_slice(b"\r\n").await?;
        for entry in entries {
            batch_writer.write_slice(b"*2\r\n").await?;
            let id_str = entry.id.format();
            batch_writer.write_bulk_string(id_str.as_bytes()).await?;

            let fields_count = entry.fields.len() * 2;
            batch_writer.write_slice(b"*").await?;
            batch_writer.write_slice(fields_count.to_string().as_bytes()).await?;
            batch_writer.write_slice(b"\r\n").await?;
            for (f, v) in entry.fields {
                batch_writer.write_bulk_string(&f).await?;
                batch_writer.write_bulk_string(&v).await?;
            }
        }
    } else if cmd.eq_ignore_ascii_case(b"COMMAND") {
        batch_writer.write_slice(b"*0\r\n").await?;
    } else {
        batch_writer.write_slice(RESP_ERR_UNKNOWN).await?;
    }

    Ok(())
}

/// Backwards-compatible convenience wrapper around `execute_command_with_repl`.
pub async fn execute_command_with_aof<W: AsyncWriteExt + Unpin>(
    frame: RespFrame<'_>,
    cache: &LocalCache,
    pubsub_hub: &PubSubHub,
    aof: &AofEngine,
    client_id: u64,
    tx: &LocalSender<PubSubMessage>,
    batch_writer: &mut BatchWriter<'_, W>,
) -> std::io::Result<()> {
    let dummy_repl = ReplicationHub::new(replication::DEFAULT_REPL_BACKLOG_SIZE);
    let dummy_acl = AclEngine::new();
    let mut conn_acl = ConnectionAcl::default();
    let mut txn_state = TransactionState::new();

        GLOBAL_ACTIVE_CONNS.fetch_add(1, Ordering::Relaxed);
        struct ConnGuard;
        impl Drop for ConnGuard {
            fn drop(&mut self) {
                GLOBAL_ACTIVE_CONNS.fetch_sub(1, Ordering::Relaxed);
            }
        }
        let _guard = ConnGuard;
    let (repl_tx, _repl_rx) = replication::replica_channel(16);
    execute_command_with_repl(
        frame,
        cache,
        pubsub_hub,
        aof,
        &dummy_repl,
        &dummy_acl,
        &mut conn_acl,
        &mut txn_state,
        client_id,
        tx,
        &repl_tx,
        batch_writer,
    )
    .await
}

/// Backwards-compatible convenience wrapper around `execute_command_with_aof`.
pub async fn execute_command_full<W: AsyncWriteExt + Unpin>(
    frame: RespFrame<'_>,
    cache: &LocalCache,
    pubsub_hub: &PubSubHub,
    client_id: u64,
    tx: &LocalSender<PubSubMessage>,
    batch_writer: &mut BatchWriter<'_, W>,
) -> std::io::Result<()> {
    let dummy_aof = AofEngine::new("default.aof", aof::AofSyncMode::No);
    execute_command_with_aof(frame, cache, pubsub_hub, &dummy_aof, client_id, tx, batch_writer).await
}

/// Executes a parsed RESP frame against the thread-local database and writes the response into `batch_writer`.
///
/// Backwards-compatible convenience wrapper around `execute_command_full`.
pub async fn execute_command_batched<W: AsyncWriteExt + Unpin>(
    frame: RespFrame<'_>,
    cache: &LocalCache,
    batch_writer: &mut BatchWriter<'_, W>,
) -> std::io::Result<()> {
    let dummy_hub = PubSubHub::new();
    let (tx, _rx) = pubsub::local_channel(16);
    execute_command_full(frame, cache, &dummy_hub, 0, &tx, batch_writer).await
}

enum StackResp {
    Formatted(usize),
    ValueTooLarge,
    NotFound,
}

#[inline]
fn stack_resp_result(len_opt: Option<Option<usize>>) -> StackResp {
    match len_opt {
        Some(Some(len)) => StackResp::Formatted(len),
        Some(None) => StackResp::ValueTooLarge,
        None => StackResp::NotFound,
    }
}

#[cfg(target_os = "linux")]
pub use linux::*;

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use crate::aof::{AlignedBuffer, AofSyncMode, DEFAULT_AOF_BUFFER_CAPACITY};
    use futures_lite::io::AsyncReadExt;
    use glommio::{
        net::{TcpListener, TcpStream},
        CpuSet, LocalExecutorBuilder, LocalExecutorPoolBuilder, Placement, PoolPlacement,
    };
    use rustls::ServerConnection;

    /// Handles a single client TCP connection on the local CPU core with aggressive batching, pub/sub, replication, and TLS/ACL.
    pub async fn handle_connection(
        raw_stream: TcpStream,
        cache: LocalCache,
        pubsub_hub: PubSubHub,
        aof: AofEngine,
        repl_hub: ReplicationHub,
        acl_engine: AclEngine,
        tls_config: Option<Arc<ServerConfig>>,
        client_id: u64,
    ) -> std::io::Result<()> {
        let core_id = glommio::executor().id();
        let (tx, mut rx) = pubsub::local_channel::<PubSubMessage>(1024);
        let (repl_tx, mut repl_rx) = replication::replica_channel(16384);
        let mut conn_acl = acl_engine.new_connection_state();
        let mut txn_state = TransactionState::new();

        // Disable Nagle's algorithm immediately to avoid 200ms Delayed ACK stall
        let _ = raw_stream.set_nodelay(true);

        let mut stream = match tls_config {
            Some(tls_cfg) => {
                let server_conn = ServerConnection::new(tls_cfg)
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
                let tls_stream = AsyncTlsStream::handshake(raw_stream, server_conn).await?;
                StreamWrapper::Tls(tls_stream)
            }
            None => StreamWrapper::Plain(raw_stream),
        };

        let mut buf = [0u8; STACK_BUFFER_SIZE];
        let mut read_pos = 0;

        loop {
            let is_subscribed = pubsub_hub.subscriber_channels_count(client_id) > 0;
            let is_replica = repl_hub.is_replica(client_id);

            let n = if is_replica {
                enum ReplEvent {
                    Socket(std::io::Result<usize>),
                    Chunk(Option<Rc<[u8]>>),
                }

                while let Some(chunk) = repl_rx.try_recv() {
                    stream.write_all(&chunk).await?;
                }
                stream.flush().await?;

                let event = futures_lite::future::race(
                    async { ReplEvent::Socket(stream.read(&mut buf[read_pos..]).await) },
                    async { ReplEvent::Chunk(repl_rx.recv().await) },
                )
                .await;

                match event {
                    ReplEvent::Socket(res) => res?,
                    ReplEvent::Chunk(Some(chunk)) => {
                        stream.write_all(&chunk).await?;
                        while let Some(extra) = repl_rx.try_recv() {
                            stream.write_all(&extra).await?;
                        }
                        stream.flush().await?;
                        continue;
                    }
                    ReplEvent::Chunk(None) => break,
                }
            } else if is_subscribed {
                enum ConnEvent {
                    Socket(std::io::Result<usize>),
                    Message(Option<PubSubMessage>),
                }

                // Drain any already queued messages first
                while let Some(msg) = rx.try_recv() {
                    let mut writer = BatchWriter::new(&mut stream);
                    writer.write_pubsub_message(&msg.channel, &msg.payload).await?;
                    writer.flush_all().await?;
                }

                let event = futures_lite::future::race(
                    async { ConnEvent::Socket(stream.read(&mut buf[read_pos..]).await) },
                    async { ConnEvent::Message(rx.recv().await) },
                )
                .await;

                match event {
                    ConnEvent::Socket(res) => res?,
                    ConnEvent::Message(Some(msg)) => {
                        let mut writer = BatchWriter::new(&mut stream);
                        writer.write_pubsub_message(&msg.channel, &msg.payload).await?;
                        while let Some(extra_msg) = rx.try_recv() {
                            writer.write_pubsub_message(&extra_msg.channel, &extra_msg.payload).await?;
                        }
                        writer.flush_all().await?;
                        continue;
                    }
                    ConnEvent::Message(None) => break,
                }
            } else {
                stream.read(&mut buf[read_pos..]).await?
            };

            if n == 0 {
                break; // Connection closed by client
            }

            read_pos += n;
            let mut batch_writer = BatchWriter::new(&mut stream);
            let mut pipeline = parser::parse_pipeline(&buf[..read_pos]);
            let mut commands_in_batch = 0;

            // Parse all available commands in the TCP buffer and batch their execution
            while let Some(res) = pipeline.next() {
                match res {
                    Ok(frame) => {
                        execute_command_with_repl(
                            frame,
                            &cache,
                            &pubsub_hub,
                            &aof,
                            &repl_hub,
                            &acl_engine,
                            &mut conn_acl,
                            &mut txn_state,
                            client_id,
                            &tx,
                            &repl_tx,
                            &mut batch_writer,
                        )
                        .await?;
                        commands_in_batch += 1;
                    }
                    Err(parser::ParseError::Incomplete) => {
                        // Incomplete frame at the end of buffer: wait for next read
                        break;
                    }
                    Err(e) => {
                        eprintln!("[Core {core_id}] Protocol parse error: {e}");
                        batch_writer.write_slice(b"-ERR Protocol error\r\n").await?;
                        batch_writer.flush_all().await?;
                        pubsub_hub.unsubscribe_all(client_id);
                        repl_hub.unregister_replica(client_id);
                        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()));
                    }
                }
            }

            // Flush all batched responses in a SINGLE socket write syscall
            if commands_in_batch > 0 || batch_writer.pending_bytes() > 0 {
                batch_writer.flush_all().await?;
                if commands_in_batch > 0 {
                    GLOBAL_TOTAL_OPS.fetch_add(commands_in_batch as u64, Ordering::Relaxed);
                }
            }

            let start = pipeline.consumed();

            // Shift unparsed partial frame bytes to the beginning of the buffer
            if start > 0 {
                buf.copy_within(start..read_pos, 0);
                read_pos -= start;
            } else if read_pos == buf.len() {
                eprintln!("[Core {core_id}] Command exceeds stack buffer capacity");
                stream.write_all(b"-ERR command too large\r\n").await?;
                stream.flush().await?;
                pubsub_hub.unsubscribe_all(client_id);
                repl_hub.unregister_replica(client_id);
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "Command frame exceeds buffer capacity",
                ));
            }
        }

        pubsub_hub.unsubscribe_all(client_id);
        repl_hub.unregister_replica(client_id);
        Ok(())
    }

    /// Serves incoming connections for an individual shard (core) via `io_uring`.
    pub async fn serve_shard(bind_addr: &str) -> std::io::Result<()> {
        let core_id = glommio::executor().id();
        let cache = LocalCache::new(); // Isolated local database per core
        let hub = PubSubHub::new(); // Isolated local pub/sub registry per core
        let repl_hub = ReplicationHub::new(replication::DEFAULT_REPL_BACKLOG_SIZE);
        let acl_engine = AclEngine::new();
        let aof_path = format!("velocity_shard_{core_id}.aof");
        let aof = AofEngine::new(&aof_path, AofSyncMode::EverySec);

        // Replay existing AOF data on shard boot
        let _ = aof.replay(&cache);

        // Spawn zero-blocking direct I/O background worker fiber
        let aof_worker = aof.clone();
        let aof_buffer = AlignedBuffer::new(DEFAULT_AOF_BUFFER_CAPACITY);
        glommio::spawn_local(async move {
            if let Err(e) = aof::run_aof_background_fiber(aof_worker, aof_buffer).await {
                eprintln!("[Core {core_id}] AOF worker fiber error: {e}");
            }
        })
        .detach();

        let listener = TcpListener::bind(bind_addr)?;
        println!("[Core {core_id}] VelocityDB shard online at {bind_addr} via io_uring");

        // Core 0 hosts the asynchronous cluster bus on (client_port + 10000)
        if core_id == 0 {
            if let Ok(client_port) = bind_addr.split(':').last().unwrap_or("6379").parse::<u16>() {
                let bus_addr = format!("0.0.0.0:{}", client_port + crate::cluster::CLUSTER_BUS_PORT_OFFSET);
                glommio::spawn_local(async move {
                    if let Ok(bus_listener) = TcpListener::bind(&bus_addr) {
                        println!("[Core 0] Cluster bus online at {bus_addr} via io_uring");
                        loop {
                            match bus_listener.accept().await {
                                Ok(stream) => {
                                    glommio::spawn_local(async move {
                                        let _ = crate::cluster::handle_cluster_bus_stream(stream).await;
                                    }).detach();
                                }
                                Err(_) => break,
                            }
                        }
                    }
                }).detach();
            }
        }
        let mut next_client_id: u64 = 1;

        loop {
            match listener.accept().await {
                Ok(stream) => {
                    let _ = stream.set_nodelay(true);
                    let local_cache = cache.clone();
                    let local_hub = hub.clone();
                    let local_aof = aof.clone();
                    let local_repl = repl_hub.clone();
                    let local_acl = acl_engine.clone();
                    let client_id = next_client_id;
                    next_client_id = next_client_id.wrapping_add(1);
                    glommio::spawn_local(async move {
                        if let Err(e) = handle_connection(
                            stream,
                            local_cache,
                            local_hub,
                            local_aof,
                            local_repl,
                            local_acl,
                            None,
                            client_id,
                        ).await {
                            eprintln!("[Core {core_id}] Client connection error: {e}");
                        }
                    })
                    .detach();
                }
                Err(e) => {
                    eprintln!("[Core {core_id}] Accept error: {e}");
                }
            }
        }
    }

    /// Spawns a dedicated `LocalExecutor` pinned to a specific CPU core.
    pub fn spawn_pinned_core<F, Fut>(
        core_id: usize,
        fut_gen: F,
    ) -> std::io::Result<glommio::ExecutorJoinHandle<()>>
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: std::future::Future<Output = ()> + 'static,
    {
        LocalExecutorBuilder::new(Placement::Fixed(core_id))
            .name(&format!("velocity-core-{core_id}"))
            .spawn(fut_gen)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
    }

    /// Launches the full Thread-per-Core TCP server across all available online CPU cores.
    pub fn run_server(bind_addr: &str) -> Result<(), Box<dyn std::error::Error>> {
        let nr_cpus = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1);

        let online_cpus = CpuSet::online().ok();
        println!("Initializing VelocityDB server on {bind_addr} across {nr_cpus} pinned CPU cores...");

        let pool = LocalExecutorPoolBuilder::new(PoolPlacement::MaxSpread(nr_cpus, online_cpus))
            .on_all_shards({
                let addr = bind_addr.to_string();
                move || async move {
                    if let Err(e) = serve_shard(&addr).await {
                        eprintln!("Shard failed on core: {e}");
                    }
                }
            })
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;

        pool.join_all();
        Ok(())
    }
}

#[cfg(not(target_os = "linux"))]
pub use non_linux::*;

#[cfg(not(target_os = "linux"))]
mod non_linux {
    use super::*;

    pub fn run_server(bind_addr: &str) -> Result<(), Box<dyn std::error::Error>> {
        println!(
            "VelocityDB production network engine uses glommio (io_uring) and requires Linux (kernel >= 5.8). \
             Requested bind on: {bind_addr} with stack buffer: {STACK_BUFFER_SIZE} bytes."
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aof::{AlignedBuffer, DEFAULT_AOF_BUFFER_CAPACITY};
    use futures_lite::io::Cursor;

    #[test]
    fn test_network_constants() {
        assert_eq!(DEFAULT_PORT, 6379);
        assert_eq!(DEFAULT_BIND_ADDR, "0.0.0.0:6379");
        assert_eq!(STACK_BUFFER_SIZE, 16384);
        assert_eq!(OUTPUT_BUFFER_SIZE, 16384);
        assert_eq!(RESP_OK, b"+OK\r\n");
        assert_eq!(RESP_PONG, b"+PONG\r\n");
        assert_eq!(RESP_NULL, b"$-1\r\n");
    }

    #[test]
    fn test_write_numbers() {
        let mut buf = [0u8; 32];
        let len1 = write_usize_to_buf(0, &mut buf);
        assert_eq!(&buf[..len1], b"0");

        let len2 = write_usize_to_buf(42, &mut buf);
        assert_eq!(&buf[..len2], b"42");

        let len3 = write_i64_to_buf(-100, &mut buf);
        assert_eq!(&buf[..len3], b"-100");
    }

    #[test]
    fn test_format_bulk_string() {
        let mut buf = [0u8; 64];
        let len = format_bulk_string_to_slice(b"hello", &mut buf).expect("format bulk");
        assert_eq!(&buf[..len], b"$5\r\nhello\r\n");
    }

    #[test]
    fn test_format_integer() {
        let mut buf = [0u8; 32];
        let len = format_integer_to_slice(1, &mut buf);
        assert_eq!(&buf[..len], b":1\r\n");

        let len2 = format_integer_to_slice(0, &mut buf);
        assert_eq!(&buf[..len2], b":0\r\n");
    }

    #[test]
    fn test_50_pipelined_commands_batching() {
        futures_lite::future::block_on(async {
            let cache = LocalCache::new();

            // Build a continuous buffer containing exactly 50 pipelined PING commands
            let mut input = Vec::with_capacity(50 * 6);
            for _ in 0..50 {
                input.extend_from_slice(b"PING\r\n");
            }

            let mut out = Cursor::new(Vec::new());
            let mut batch_writer = BatchWriter::new(&mut out);

            let mut pipeline = parser::parse_pipeline(&input);
            let mut count = 0;

            while let Some(Ok(frame)) = pipeline.next() {
                execute_command_batched(frame, &cache, &mut batch_writer)
                    .await
                    .expect("execute batched ping");
                count += 1;
            }

            assert_eq!(count, 50);
            assert_eq!(pipeline.consumed(), input.len());

            // Prior to flush_all, all 50 * 7 = 350 bytes reside in the batch buffer
            assert_eq!(batch_writer.pending_bytes(), 50 * 7);

            // Single batch flush commits all 50 responses simultaneously
            batch_writer.flush_all().await.expect("flush batch");
            assert_eq!(batch_writer.pending_bytes(), 0);

            let result = out.into_inner();
            let mut expected = Vec::with_capacity(50 * 7);
            for _ in 0..50 {
                expected.extend_from_slice(b"+PONG\r\n");
            }
            assert_eq!(result, expected);
        });
    }

    #[test]
    fn test_mixed_pipelined_commands_batching() {
        futures_lite::future::block_on(async {
            let cache = LocalCache::new();
            let input = b"SET alpha 100\r\nGET alpha\r\nDEL alpha\r\nGET alpha\r\n";

            let mut out = Cursor::new(Vec::new());
            let mut batch_writer = BatchWriter::new(&mut out);

            let mut pipeline = parser::parse_pipeline(input);
            while let Some(Ok(frame)) = pipeline.next() {
                execute_command_batched(frame, &cache, &mut batch_writer)
                    .await
                    .expect("execute command");
            }

            batch_writer.flush_all().await.expect("flush batch");
            assert_eq!(out.into_inner(), b"+OK\r\n$3\r\n100\r\n:1\r\n$-1\r\n");
        });
    }

    #[test]
    fn test_execute_set_with_ttl() {
        futures_lite::future::block_on(async {
            let cache = LocalCache::new();
            let mut out = Cursor::new(Vec::new());
            let mut batch_writer = BatchWriter::new(&mut out);

            let (frame, _) = parser::parse(b"SET temp val EX 60\r\n").expect("parse set ex");
            execute_command_batched(frame, &cache, &mut batch_writer)
                .await
                .expect("exec set ex");
            batch_writer.flush_all().await.expect("flush");

            assert_eq!(out.into_inner(), b"+OK\r\n");
            assert!(cache.ttl(b"temp").expect("ttl").is_some());
        });
    }

    #[test]
    fn test_batch_writer_pubsub_formatting() {
        futures_lite::future::block_on(async {
            let mut out = Cursor::new(Vec::new());
            let mut writer = BatchWriter::new(&mut out);

            writer.write_pubsub_subscribe(b"sports", 1).await.unwrap();
            writer.write_pubsub_unsubscribe(Some(b"sports"), 0).await.unwrap();
            writer.write_pubsub_message(b"sports", b"goal!").await.unwrap();
            writer.flush_all().await.unwrap();

            let bytes = out.into_inner();
            let expected = b"*3\r\n$9\r\nsubscribe\r\n$6\r\nsports\r\n:1\r\n\
                             *3\r\n$11\r\nunsubscribe\r\n$6\r\nsports\r\n:0\r\n\
                             *3\r\n$7\r\nmessage\r\n$6\r\nsports\r\n$5\r\ngoal!\r\n";
            assert_eq!(bytes, expected);
        });
    }

    #[test]
    fn test_pubsub_subscribe_and_publish_execution() {
        futures_lite::future::block_on(async {
            let cache = LocalCache::new();
            let hub = PubSubHub::new();
            let (tx1, mut rx1) = pubsub::local_channel(16);

            // Client 1: SUBSCRIBE channel_a channel_b
            let mut out1 = Cursor::new(Vec::new());
            let mut writer1 = BatchWriter::new(&mut out1);
            let (f1, _) = parser::parse(b"SUBSCRIBE channel_a channel_b\r\n").unwrap();
            execute_command_full(f1, &cache, &hub, 100, &tx1, &mut writer1).await.unwrap();
            writer1.flush_all().await.unwrap();

            let expected_sub = b"*3\r\n$9\r\nsubscribe\r\n$9\r\nchannel_a\r\n:1\r\n\
                                 *3\r\n$9\r\nsubscribe\r\n$9\r\nchannel_b\r\n:2\r\n";
            assert_eq!(out1.into_inner(), expected_sub);

            // Client 2: PUBLISH channel_a "hello world"
            let (tx2, _rx2) = pubsub::local_channel(16);
            let mut out2 = Cursor::new(Vec::new());
            let mut writer2 = BatchWriter::new(&mut out2);
            let (f2, _) = parser::parse(b"PUBLISH channel_a hello_world\r\n").unwrap();
            execute_command_full(f2, &cache, &hub, 200, &tx2, &mut writer2).await.unwrap();
            writer2.flush_all().await.unwrap();

            // Client 2 receives integer response: 1 subscriber reached
            assert_eq!(out2.into_inner(), b":1\r\n");

            // Client 1 receives pubsub message via rx1
            let msg = rx1.try_recv().expect("client 1 must receive message");
            assert_eq!(msg.channel.as_ref(), b"channel_a");
            assert_eq!(msg.payload.as_ref(), b"hello_world");
        });
    }

    #[test]
    fn test_pubsub_unsubscribe_execution() {
        futures_lite::future::block_on(async {
            let cache = LocalCache::new();
            let hub = PubSubHub::new();
            let (tx, _rx) = pubsub::local_channel(16);

            let mut out = Cursor::new(Vec::new());
            let mut writer = BatchWriter::new(&mut out);

            // SUBSCRIBE
            let (f1, _) = parser::parse(b"SUBSCRIBE ch1\r\n").unwrap();
            execute_command_full(f1, &cache, &hub, 1, &tx, &mut writer).await.unwrap();

            // UNSUBSCRIBE ch1
            let (f2, _) = parser::parse(b"UNSUBSCRIBE ch1\r\n").unwrap();
            execute_command_full(f2, &cache, &hub, 1, &tx, &mut writer).await.unwrap();
            writer.flush_all().await.unwrap();

            let expected = b"*3\r\n$9\r\nsubscribe\r\n$3\r\nch1\r\n:1\r\n\
                             *3\r\n$11\r\nunsubscribe\r\n$3\r\nch1\r\n:0\r\n";
            assert_eq!(out.into_inner(), expected);
            assert_eq!(hub.subscribers_count(b"ch1"), 0);
        });
    }

    #[test]
    fn test_execute_command_with_aof_staging() {
        futures_lite::future::block_on(async {
            let cache = LocalCache::new();
            let hub = PubSubHub::new();
            let aof = AofEngine::new("test_net_aof.aof", aof::AofSyncMode::No);
            let (tx, _rx) = pubsub::local_channel(16);

            let mut out = Cursor::new(Vec::new());
            let mut writer = BatchWriter::new(&mut out);

            // Execute SET
            let (f1, _) = parser::parse(b"SET persistent_key val123\r\n").unwrap();
            execute_command_with_aof(f1, &cache, &hub, &aof, 1, &tx, &mut writer).await.unwrap();
            writer.flush_all().await.unwrap();

            assert_eq!(out.into_inner(), b"+OK\r\n");
            assert_eq!(cache.get(b"persistent_key"), Some(b"val123".to_vec()));
            assert_eq!(aof.pending_records(), 1);

            // Drain into aligned buffer
            let mut buf = AlignedBuffer::new(DEFAULT_AOF_BUFFER_CAPACITY);
            let count = aof.drain_to_aligned_buffer(&mut buf);
            assert_eq!(count, 1);
            assert_eq!(buf.len(), b"*3\r\n$3\r\nSET\r\n$14\r\npersistent_key\r\n$6\r\nval123\r\n".len());
        });
    }

    #[test]
    fn test_replication_psync_fullresync_and_continue() {
        futures_lite::future::block_on(async {
            let cache = LocalCache::new();
            cache.set(b"init_key", b"init_val");
            let pubsub_hub = PubSubHub::new();
            let aof = AofEngine::new("test_repl.aof", aof::AofSyncMode::No);
            let repl_hub = ReplicationHub::new(1024);
            let acl_engine = AclEngine::new();
            let mut conn_acl = ConnectionAcl::default();
            let mut txn_state = TransactionState::new();
            let (tx, _rx) = pubsub::local_channel(16);
            let (repl_tx, mut repl_rx) = replication::replica_channel(16);
            let client_id = 99;

            let mut out = Cursor::new(Vec::new());
            let mut writer = BatchWriter::new(&mut out);

            // 1. Replica sends PSYNC ? -1 -> must trigger FULLRESYNC
            let (f1, _) = parser::parse(b"PSYNC ? -1\r\n").unwrap();
            execute_command_with_repl(f1, &cache, &pubsub_hub, &aof, &repl_hub, &acl_engine, &mut conn_acl, &mut txn_state, client_id, &tx, &repl_tx, &mut writer)
                .await
                .unwrap();
            writer.flush_all().await.unwrap();

            let resp1 = String::from_utf8_lossy(&out.into_inner()).into_owned();
            assert!(resp1.contains("+FULLRESYNC"));
            assert!(resp1.contains("init_key"));
            assert!(resp1.contains("init_val"));

            // Replica is now online
            assert!(repl_hub.is_replica(client_id));

            // 2. Execute a mutating SET command on Master
            let mut out2 = Cursor::new(Vec::new());
            let mut writer2 = BatchWriter::new(&mut out2);
            let (f2, _) = parser::parse(b"SET live_user bob\r\n").unwrap();
            execute_command_with_repl(f2, &cache, &pubsub_hub, &aof, &repl_hub, &acl_engine, &mut conn_acl, &mut txn_state, 1, &tx, &repl_tx, &mut writer2)
                .await
                .unwrap();
            writer2.flush_all().await.unwrap();
            assert_eq!(out2.into_inner(), b"+OK\r\n");

            // Verify replica channel received the live broadcast chunk
            let chunk = repl_rx.try_recv().expect("replica received live stream");
            assert_eq!(chunk.as_ref(), b"*3\r\n$3\r\nSET\r\n$9\r\nlive_user\r\n$3\r\nbob\r\n");

            // 3. New replica connects and requests Partial Resync at offset 0
            let (repl_tx2, _repl_rx2) = replication::replica_channel(16);
            let client_id2 = 101;
            let mut conn_acl2 = ConnectionAcl::default();
            let mut txn_state2 = TransactionState::new();
            let mut out3 = Cursor::new(Vec::new());
            let mut writer3 = BatchWriter::new(&mut out3);

            let replid = repl_hub.master_replid_str();
            let psync_cmd = format!("PSYNC {} 0\r\n", replid);
            let (f3, _) = parser::parse(psync_cmd.as_bytes()).unwrap();
            execute_command_with_repl(f3, &cache, &pubsub_hub, &aof, &repl_hub, &acl_engine, &mut conn_acl2, &mut txn_state2, client_id2, &tx, &repl_tx2, &mut writer3)
                .await
                .unwrap();
            writer3.flush_all().await.unwrap();

            let resp3 = String::from_utf8_lossy(&out3.into_inner()).into_owned();
            assert!(resp3.contains("+CONTINUE"));
            assert!(resp3.contains("live_user"));
        });
    }

    #[test]
    fn test_replication_replconf_and_role() {
        futures_lite::future::block_on(async {
            let cache = LocalCache::new();
            let pubsub_hub = PubSubHub::new();
            let aof = AofEngine::new("test_replconf.aof", aof::AofSyncMode::No);
            let repl_hub = ReplicationHub::new(1024);
            let acl_engine = AclEngine::new();
            let mut conn_acl = ConnectionAcl::default();
            let mut txn_state = TransactionState::new();
            let (tx, _rx) = pubsub::local_channel(16);
            let (repl_tx, _repl_rx) = replication::replica_channel(16);
            let client_id = 77;

            let mut out = Cursor::new(Vec::new());
            let mut writer = BatchWriter::new(&mut out);

            // REPLCONF listening-port 6388
            let (f1, _) = parser::parse(b"REPLCONF listening-port 6388\r\n").unwrap();
            execute_command_with_repl(f1, &cache, &pubsub_hub, &aof, &repl_hub, &acl_engine, &mut conn_acl, &mut txn_state, client_id, &tx, &repl_tx, &mut writer).await.unwrap();

            // REPLCONF capa psync2
            let (f2, _) = parser::parse(b"REPLCONF capa psync2\r\n").unwrap();
            execute_command_with_repl(f2, &cache, &pubsub_hub, &aof, &repl_hub, &acl_engine, &mut conn_acl, &mut txn_state, client_id, &tx, &repl_tx, &mut writer).await.unwrap();

            // REPLCONF ACK 1234
            let (f3, _) = parser::parse(b"REPLCONF ACK 1234\r\n").unwrap();
            execute_command_with_repl(f3, &cache, &pubsub_hub, &aof, &repl_hub, &acl_engine, &mut conn_acl, &mut txn_state, client_id, &tx, &repl_tx, &mut writer).await.unwrap();

            // ROLE command
            let (f4, _) = parser::parse(b"ROLE\r\n").unwrap();
            execute_command_with_repl(f4, &cache, &pubsub_hub, &aof, &repl_hub, &acl_engine, &mut conn_acl, &mut txn_state, client_id, &tx, &repl_tx, &mut writer).await.unwrap();

            // INFO replication
            let (f5, _) = parser::parse(b"INFO replication\r\n").unwrap();
            execute_command_with_repl(f5, &cache, &pubsub_hub, &aof, &repl_hub, &acl_engine, &mut conn_acl, &mut txn_state, client_id, &tx, &repl_tx, &mut writer).await.unwrap();

            writer.flush_all().await.unwrap();

            let out_str = String::from_utf8_lossy(&out.into_inner()).into_owned();
            assert!(out_str.contains("+OK\r\n"));
            assert!(out_str.contains("master"));
            assert!(out_str.contains("6388"));
            assert!(out_str.contains("role:master"));
            assert!(out_str.contains("connected_slaves:1"));
        });
    }

    #[test]
    fn test_acl_authentication_and_authorization_flow() {
        futures_lite::future::block_on(async {
            let cache = LocalCache::new();
            let pubsub_hub = PubSubHub::new();
            let aof = AofEngine::new("test_acl.aof", aof::AofSyncMode::No);
            let repl_hub = ReplicationHub::new(1024);
            let acl_engine = AclEngine::new();
            let mut txn_state = TransactionState::new();
            let (tx, _rx) = pubsub::local_channel(16);
            let (repl_tx, _repl_rx) = replication::replica_channel(16);
            let client_id = 1;

            // Start with unauthenticated connection
            let mut conn_acl = ConnectionAcl::unauthenticated();

            // 1. Attempt GET without auth -> must fail with -NOAUTH
            let mut out1 = Cursor::new(Vec::new());
            let mut w1 = BatchWriter::new(&mut out1);
            let (f1, _) = parser::parse(b"GET account:123\r\n").unwrap();
            execute_command_with_repl(f1, &cache, &pubsub_hub, &aof, &repl_hub, &acl_engine, &mut conn_acl, &mut txn_state, client_id, &tx, &repl_tx, &mut w1).await.unwrap();
            w1.flush_all().await.unwrap();
            assert_eq!(out1.into_inner(), b"-NOAUTH Authentication required.\r\n");

            // 2. PING is a CAT_CONNECTION command -> must succeed even before auth
            let mut out2 = Cursor::new(Vec::new());
            let mut w2 = BatchWriter::new(&mut out2);
            let (f2, _) = parser::parse(b"PING\r\n").unwrap();
            execute_command_with_repl(f2, &cache, &pubsub_hub, &aof, &repl_hub, &acl_engine, &mut conn_acl, &mut txn_state, client_id, &tx, &repl_tx, &mut w2).await.unwrap();
            w2.flush_all().await.unwrap();
            assert_eq!(out2.into_inner(), b"+PONG\r\n");

            // 3. Authenticate with bad password -> -WRONGPASS
            let mut out3 = Cursor::new(Vec::new());
            let mut w3 = BatchWriter::new(&mut out3);
            let (f3, _) = parser::parse(b"AUTH fintech_auditor wrong_password\r\n").unwrap();
            execute_command_with_repl(f3, &cache, &pubsub_hub, &aof, &repl_hub, &acl_engine, &mut conn_acl, &mut txn_state, client_id, &tx, &repl_tx, &mut w3).await.unwrap();
            w3.flush_all().await.unwrap();
            assert!(out3.into_inner().starts_with(b"-WRONGPASS"));

            // 4. Authenticate as fintech_auditor (read-only) -> +OK
            let mut out4 = Cursor::new(Vec::new());
            let mut w4 = BatchWriter::new(&mut out4);
            let (f4, _) = parser::parse(b"AUTH fintech_auditor Audit#ReadOnly2026\r\n").unwrap();
            execute_command_with_repl(f4, &cache, &pubsub_hub, &aof, &repl_hub, &acl_engine, &mut conn_acl, &mut txn_state, client_id, &tx, &repl_tx, &mut w4).await.unwrap();
            w4.flush_all().await.unwrap();
            assert_eq!(out4.into_inner(), b"+OK\r\n");
            assert_eq!(conn_acl.username, "fintech_auditor");

            // 5. Check WHOAMI
            let mut out5 = Cursor::new(Vec::new());
            let mut w5 = BatchWriter::new(&mut out5);
            let (f5, _) = parser::parse(b"ACL WHOAMI\r\n").unwrap();
            execute_command_with_repl(f5, &cache, &pubsub_hub, &aof, &repl_hub, &acl_engine, &mut conn_acl, &mut txn_state, client_id, &tx, &repl_tx, &mut w5).await.unwrap();
            w5.flush_all().await.unwrap();
            assert_eq!(out5.into_inner(), b"$15\r\nfintech_auditor\r\n");

            // 6. Auditor attempts SET (write) -> -NOPERM
            let mut out6 = Cursor::new(Vec::new());
            let mut w6 = BatchWriter::new(&mut out6);
            let (f6, _) = parser::parse(b"SET balance 1000000\r\n").unwrap();
            execute_command_with_repl(f6, &cache, &pubsub_hub, &aof, &repl_hub, &acl_engine, &mut conn_acl, &mut txn_state, client_id, &tx, &repl_tx, &mut w6).await.unwrap();
            w6.flush_all().await.unwrap();
            assert_eq!(out6.into_inner(), b"-NOPERM this user has no permissions to run the command\r\n");

            // 7. Authenticate as fintech_admin -> +OK
            let mut out7 = Cursor::new(Vec::new());
            let mut w7 = BatchWriter::new(&mut out7);
            let (f7, _) = parser::parse(b"AUTH fintech_admin FinTech#2026!Admin\r\n").unwrap();
            execute_command_with_repl(f7, &cache, &pubsub_hub, &aof, &repl_hub, &acl_engine, &mut conn_acl, &mut txn_state, client_id, &tx, &repl_tx, &mut w7).await.unwrap();
            w7.flush_all().await.unwrap();
            assert_eq!(out7.into_inner(), b"+OK\r\n");

            // 8. Admin executes SET balance -> +OK
            let mut out8 = Cursor::new(Vec::new());
            let mut w8 = BatchWriter::new(&mut out8);
            let (f8, _) = parser::parse(b"SET balance 1000000\r\n").unwrap();
            execute_command_with_repl(f8, &cache, &pubsub_hub, &aof, &repl_hub, &acl_engine, &mut conn_acl, &mut txn_state, client_id, &tx, &repl_tx, &mut w8).await.unwrap();
            w8.flush_all().await.unwrap();
            assert_eq!(out8.into_inner(), b"+OK\r\n");

            // 9. Admin creates a new custom role via ACL SETUSER
            let mut out9 = Cursor::new(Vec::new());
            let mut w9 = BatchWriter::new(&mut out9);
            let (f9, _) = parser::parse(b"ACL SETUSER risk_engine >SafeRisk#99 +@read +@write\r\n").unwrap();
            execute_command_with_repl(f9, &cache, &pubsub_hub, &aof, &repl_hub, &acl_engine, &mut conn_acl, &mut txn_state, client_id, &tx, &repl_tx, &mut w9).await.unwrap();
            w9.flush_all().await.unwrap();
            assert_eq!(out9.into_inner(), b"+OK\r\n");

            // 10. Query ACL USERS -> contains risk_engine
            let mut out10 = Cursor::new(Vec::new());
            let mut w10 = BatchWriter::new(&mut out10);
            let (f10, _) = parser::parse(b"ACL USERS\r\n").unwrap();
            execute_command_with_repl(f10, &cache, &pubsub_hub, &aof, &repl_hub, &acl_engine, &mut conn_acl, &mut txn_state, client_id, &tx, &repl_tx, &mut w10).await.unwrap();
            w10.flush_all().await.unwrap();
            let users_resp = String::from_utf8_lossy(&out10.into_inner()).into_owned();
            assert!(users_resp.contains("risk_engine"));

            // 11. Delete user via ACL DELUSER
            let mut out11 = Cursor::new(Vec::new());
            let mut w11 = BatchWriter::new(&mut out11);
            let (f11, _) = parser::parse(b"ACL DELUSER risk_engine\r\n").unwrap();
            execute_command_with_repl(f11, &cache, &pubsub_hub, &aof, &repl_hub, &acl_engine, &mut conn_acl, &mut txn_state, client_id, &tx, &repl_tx, &mut w11).await.unwrap();
            w11.flush_all().await.unwrap();
            assert_eq!(out11.into_inner(), b":1\r\n");
        });
    }

    #[test]
    fn test_tls_configuration_and_stream_wrapper() {
        futures_lite::future::block_on(async {
            // Verify TLS config generation with Ring crypto and self-signed cert
            let tls_config = create_test_tls_config().expect("TLS config generation must succeed");
            assert!(tls_config.alpn_protocols.is_empty());

            // Verify StreamWrapper::Plain with in-memory AsyncRead/AsyncWrite Cursor
            let mut inner = Cursor::new(Vec::new());
            let mut plain_wrapper = StreamWrapper::Plain(&mut inner);

            plain_wrapper.write_all(b"+PONG\r\n").await.unwrap();
            plain_wrapper.flush().await.unwrap();

            assert_eq!(inner.into_inner(), b"+PONG\r\n");
        });
    }

    #[test]
    fn test_bitmaps_setbit_getbit_bitcount() {
        futures_lite::future::block_on(async {
            let cache = LocalCache::new();
            let pubsub_hub = PubSubHub::new();
            let aof = AofEngine::new("test_bitmap.aof", aof::AofSyncMode::No);
            let repl_hub = ReplicationHub::new(1024);
            let acl_engine = AclEngine::new();
            let mut conn_acl = ConnectionAcl::default();
            let mut txn_state = TransactionState::new();
            let (tx, _rx) = pubsub::local_channel(16);
            let (repl_tx, _repl_rx) = replication::replica_channel(16);

            // SETBIT flags 0 1 -> 0
            let mut out1 = Cursor::new(Vec::new());
            let mut w1 = BatchWriter::new(&mut out1);
            let (f1, _) = parser::parse(b"SETBIT flags 0 1\r\n").unwrap();
            execute_command_with_repl(f1, &cache, &pubsub_hub, &aof, &repl_hub, &acl_engine, &mut conn_acl, &mut txn_state, 1, &tx, &repl_tx, &mut w1).await.unwrap();
            w1.flush_all().await.unwrap();
            assert_eq!(out1.into_inner(), b":0\r\n");

            // GETBIT flags 0 -> 1
            let mut out2 = Cursor::new(Vec::new());
            let mut w2 = BatchWriter::new(&mut out2);
            let (f2, _) = parser::parse(b"GETBIT flags 0\r\n").unwrap();
            execute_command_with_repl(f2, &cache, &pubsub_hub, &aof, &repl_hub, &acl_engine, &mut conn_acl, &mut txn_state, 1, &tx, &repl_tx, &mut w2).await.unwrap();
            w2.flush_all().await.unwrap();
            assert_eq!(out2.into_inner(), b":1\r\n");

            // GETBIT flags 1 -> 0
            let mut out3 = Cursor::new(Vec::new());
            let mut w3 = BatchWriter::new(&mut out3);
            let (f3, _) = parser::parse(b"GETBIT flags 1\r\n").unwrap();
            execute_command_with_repl(f3, &cache, &pubsub_hub, &aof, &repl_hub, &acl_engine, &mut conn_acl, &mut txn_state, 1, &tx, &repl_tx, &mut w3).await.unwrap();
            w3.flush_all().await.unwrap();
            assert_eq!(out3.into_inner(), b":0\r\n");

            // SETBIT flags 7 1 -> 0, SETBIT flags 15 1 -> 0
            let (f4, _) = parser::parse(b"SETBIT flags 7 1\r\n").unwrap();
            let mut out4 = Cursor::new(Vec::new());
            let mut w4 = BatchWriter::new(&mut out4);
            execute_command_with_repl(f4, &cache, &pubsub_hub, &aof, &repl_hub, &acl_engine, &mut conn_acl, &mut txn_state, 1, &tx, &repl_tx, &mut w4).await.unwrap();
            w4.flush_all().await.unwrap();

            let (f5, _) = parser::parse(b"SETBIT flags 15 1\r\n").unwrap();
            let mut out5 = Cursor::new(Vec::new());
            let mut w5 = BatchWriter::new(&mut out5);
            execute_command_with_repl(f5, &cache, &pubsub_hub, &aof, &repl_hub, &acl_engine, &mut conn_acl, &mut txn_state, 1, &tx, &repl_tx, &mut w5).await.unwrap();
            w5.flush_all().await.unwrap();

            // BITCOUNT flags -> 3
            let mut out6 = Cursor::new(Vec::new());
            let mut w6 = BatchWriter::new(&mut out6);
            let (f6, _) = parser::parse(b"BITCOUNT flags\r\n").unwrap();
            execute_command_with_repl(f6, &cache, &pubsub_hub, &aof, &repl_hub, &acl_engine, &mut conn_acl, &mut txn_state, 1, &tx, &repl_tx, &mut w6).await.unwrap();
            w6.flush_all().await.unwrap();
            assert_eq!(out6.into_inner(), b":3\r\n");

            // Clear bit 0: SETBIT flags 0 0 -> returns previous value 1
            let mut out7 = Cursor::new(Vec::new());
            let mut w7 = BatchWriter::new(&mut out7);
            let (f7, _) = parser::parse(b"SETBIT flags 0 0\r\n").unwrap();
            execute_command_with_repl(f7, &cache, &pubsub_hub, &aof, &repl_hub, &acl_engine, &mut conn_acl, &mut txn_state, 1, &tx, &repl_tx, &mut w7).await.unwrap();
            w7.flush_all().await.unwrap();
            assert_eq!(out7.into_inner(), b":1\r\n");

            // BITCOUNT flags -> 2
            let mut out8 = Cursor::new(Vec::new());
            let mut w8 = BatchWriter::new(&mut out8);
            let (f8, _) = parser::parse(b"BITCOUNT flags\r\n").unwrap();
            execute_command_with_repl(f8, &cache, &pubsub_hub, &aof, &repl_hub, &acl_engine, &mut conn_acl, &mut txn_state, 1, &tx, &repl_tx, &mut w8).await.unwrap();
            w8.flush_all().await.unwrap();
            assert_eq!(out8.into_inner(), b":2\r\n");
        });
    }

    #[test]
    fn test_streams_xadd_xread_xlen_xrange() {
        futures_lite::future::block_on(async {
            let cache = LocalCache::new();
            let pubsub_hub = PubSubHub::new();
            let aof = AofEngine::new("test_stream.aof", aof::AofSyncMode::No);
            let repl_hub = ReplicationHub::new(1024);
            let acl_engine = AclEngine::new();
            let mut conn_acl = ConnectionAcl::default();
            let mut txn_state = TransactionState::new();
            let (tx, _rx) = pubsub::local_channel(16);
            let (repl_tx, _repl_rx) = replication::replica_channel(16);

            // 1. XADD events 1000-0 sensor temp
            let mut out1 = Cursor::new(Vec::new());
            let mut w1 = BatchWriter::new(&mut out1);
            let (f1, _) = parser::parse(b"XADD events 1000-0 sensor temp\r\n").unwrap();
            execute_command_with_repl(f1, &cache, &pubsub_hub, &aof, &repl_hub, &acl_engine, &mut conn_acl, &mut txn_state, 1, &tx, &repl_tx, &mut w1).await.unwrap();
            w1.flush_all().await.unwrap();
            assert_eq!(out1.into_inner(), b"$6\r\n1000-0\r\n");

            // 2. XADD events 1000-1 sensor humidity
            let mut out2 = Cursor::new(Vec::new());
            let mut w2 = BatchWriter::new(&mut out2);
            let (f2, _) = parser::parse(b"XADD events 1000-1 sensor humidity\r\n").unwrap();
            execute_command_with_repl(f2, &cache, &pubsub_hub, &aof, &repl_hub, &acl_engine, &mut conn_acl, &mut txn_state, 1, &tx, &repl_tx, &mut w2).await.unwrap();
            w2.flush_all().await.unwrap();
            assert_eq!(out2.into_inner(), b"$6\r\n1000-1\r\n");

            // 3. XLEN events -> 2
            let mut out3 = Cursor::new(Vec::new());
            let mut w3 = BatchWriter::new(&mut out3);
            let (f3, _) = parser::parse(b"XLEN events\r\n").unwrap();
            execute_command_with_repl(f3, &cache, &pubsub_hub, &aof, &repl_hub, &acl_engine, &mut conn_acl, &mut txn_state, 1, &tx, &repl_tx, &mut w3).await.unwrap();
            w3.flush_all().await.unwrap();
            assert_eq!(out3.into_inner(), b":2\r\n");

            // 4. Invalid smaller ID -> must return error
            let mut out4 = Cursor::new(Vec::new());
            let mut w4 = BatchWriter::new(&mut out4);
            let (f4, _) = parser::parse(b"XADD events 1000-0 sensor temp\r\n").unwrap();
            execute_command_with_repl(f4, &cache, &pubsub_hub, &aof, &repl_hub, &acl_engine, &mut conn_acl, &mut txn_state, 1, &tx, &repl_tx, &mut w4).await.unwrap();
            w4.flush_all().await.unwrap();
            let err_resp = String::from_utf8_lossy(&out4.into_inner()).into_owned();
            assert!(err_resp.contains("-ERR The ID specified in XADD is equal or smaller"));

            // 5. XREAD STREAMS events 0-0 -> reads both entries
            let mut out5 = Cursor::new(Vec::new());
            let mut w5 = BatchWriter::new(&mut out5);
            let (f5, _) = parser::parse(b"XREAD STREAMS events 0-0\r\n").unwrap();
            execute_command_with_repl(f5, &cache, &pubsub_hub, &aof, &repl_hub, &acl_engine, &mut conn_acl, &mut txn_state, 1, &tx, &repl_tx, &mut w5).await.unwrap();
            w5.flush_all().await.unwrap();
            let xread_resp = String::from_utf8_lossy(&out5.into_inner()).into_owned();
            assert!(xread_resp.contains("events"));
            assert!(xread_resp.contains("1000-0"));
            assert!(xread_resp.contains("1000-1"));

            // 6. XRANGE events - + -> returns entries
            let mut out6 = Cursor::new(Vec::new());
            let mut w6 = BatchWriter::new(&mut out6);
            let (f6, _) = parser::parse(b"XRANGE events - +\r\n").unwrap();
            execute_command_with_repl(f6, &cache, &pubsub_hub, &aof, &repl_hub, &acl_engine, &mut conn_acl, &mut txn_state, 1, &tx, &repl_tx, &mut w6).await.unwrap();
            w6.flush_all().await.unwrap();
            let xrange_resp = String::from_utf8_lossy(&out6.into_inner()).into_owned();
            assert!(xrange_resp.contains("1000-0"));
            assert!(xrange_resp.contains("1000-1"));
        });
    }

    #[test]
    fn test_multi_exec_deterministic_transactions_with_hash_tagging() {
        futures_lite::future::block_on(async {
            let cache = LocalCache::new();
            let pubsub_hub = PubSubHub::new();
            let aof = AofEngine::new("test_txn.aof", aof::AofSyncMode::No);
            let repl_hub = ReplicationHub::new(1024);
            let acl_engine = AclEngine::new();
            let mut conn_acl = ConnectionAcl::default();
            let mut txn_state = TransactionState::new();
            let (tx, _rx) = pubsub::local_channel(16);
            let (repl_tx, _repl_rx) = replication::replica_channel(16);
            let client_id = 1;

            // Pipeline: MULTI, SET {user1}name Alice, GET {user1}name, SETBIT {user1}active 0 1, EXEC
            let pipeline_bytes = b"MULTI\r\nSET {user1}name Alice\r\nGET {user1}name\r\nSETBIT {user1}active 0 1\r\nEXEC\r\n";
            let mut out = Cursor::new(Vec::new());
            let mut writer = BatchWriter::new(&mut out);

            let mut pipeline = parser::parse_pipeline(pipeline_bytes);
            while let Some(res) = pipeline.next() {
                let frame = res.unwrap();
                execute_command_with_repl(frame, &cache, &pubsub_hub, &aof, &repl_hub, &acl_engine, &mut conn_acl, &mut txn_state, client_id, &tx, &repl_tx, &mut writer).await.unwrap();
            }
            writer.flush_all().await.unwrap();

            let resp = String::from_utf8_lossy(&out.into_inner()).into_owned();
            // Responses: +OK (MULTI), +QUEUED, +QUEUED, +QUEUED, *3\r\n+OK\r\n$5\r\nAlice\r\n:0\r\n (EXEC)
            assert!(resp.contains("+OK\r\n"));
            assert!(resp.contains("+QUEUED\r\n"));
            assert!(resp.contains("*3\r\n"));
            assert!(resp.contains("Alice"));
            assert_eq!(cache.get(b"{user1}name"), Some(b"Alice".to_vec()));
            assert_eq!(cache.getbit(b"{user1}active", 0), 1);
        });
    }

    #[test]
    fn test_multi_exec_cross_core_rejection_with_hash_tagging() {
        futures_lite::future::block_on(async {
            let cache = LocalCache::new();
            let pubsub_hub = PubSubHub::new();
            let aof = AofEngine::new("test_crossslot.aof", aof::AofSyncMode::No);
            let repl_hub = ReplicationHub::new(1024);
            let acl_engine = AclEngine::new();
            let mut conn_acl = ConnectionAcl::default();
            let mut txn_state = TransactionState::new();
            let (tx, _rx) = pubsub::local_channel(16);
            let (repl_tx, _repl_rx) = replication::replica_channel(16);
            let client_id = 1;

            // MULTI
            let mut out1 = Cursor::new(Vec::new());
            let mut w1 = BatchWriter::new(&mut out1);
            let (f1, _) = parser::parse(b"MULTI\r\n").unwrap();
            execute_command_with_repl(f1, &cache, &pubsub_hub, &aof, &repl_hub, &acl_engine, &mut conn_acl, &mut txn_state, client_id, &tx, &repl_tx, &mut w1).await.unwrap();
            w1.flush_all().await.unwrap();
            assert_eq!(out1.into_inner(), b"+OK\r\n");

            // SET {user1}account 1000 -> Queued with tag user1
            let mut out2 = Cursor::new(Vec::new());
            let mut w2 = BatchWriter::new(&mut out2);
            let (f2, _) = parser::parse(b"SET {user1}account 1000\r\n").unwrap();
            execute_command_with_repl(f2, &cache, &pubsub_hub, &aof, &repl_hub, &acl_engine, &mut conn_acl, &mut txn_state, client_id, &tx, &repl_tx, &mut w2).await.unwrap();
            w2.flush_all().await.unwrap();
            assert_eq!(out2.into_inner(), b"+QUEUED\r\n");

            // SET {user2}account 2000 -> Keys hash to different cores! MUST REJECT IMMEDIATELY
            let mut out3 = Cursor::new(Vec::new());
            let mut w3 = BatchWriter::new(&mut out3);
            let (f3, _) = parser::parse(b"SET {user2}account 2000\r\n").unwrap();
            execute_command_with_repl(f3, &cache, &pubsub_hub, &aof, &repl_hub, &acl_engine, &mut conn_acl, &mut txn_state, client_id, &tx, &repl_tx, &mut w3).await.unwrap();
            w3.flush_all().await.unwrap();
            assert_eq!(out3.into_inner(), b"-CROSSSLOT Keys in MULTI block hash to different cores\r\n");

            // EXEC -> Must abort with -EXECABORT
            let mut out4 = Cursor::new(Vec::new());
            let mut w4 = BatchWriter::new(&mut out4);
            let (f4, _) = parser::parse(b"EXEC\r\n").unwrap();
            execute_command_with_repl(f4, &cache, &pubsub_hub, &aof, &repl_hub, &acl_engine, &mut conn_acl, &mut txn_state, client_id, &tx, &repl_tx, &mut w4).await.unwrap();
            w4.flush_all().await.unwrap();
            assert_eq!(out4.into_inner(), b"-EXECABORT Transaction discarded because of cross-slot keys\r\n");

            // Verify neither key was modified
            assert_eq!(cache.get(b"{user1}account"), None);
            assert_eq!(cache.get(b"{user2}account"), None);
        });
    }

    #[test]
    fn test_multi_discard() {
        futures_lite::future::block_on(async {
            let cache = LocalCache::new();
            let pubsub_hub = PubSubHub::new();
            let aof = AofEngine::new("test_discard.aof", aof::AofSyncMode::No);
            let repl_hub = ReplicationHub::new(1024);
            let acl_engine = AclEngine::new();
            let mut conn_acl = ConnectionAcl::default();
            let mut txn_state = TransactionState::new();
            let (tx, _rx) = pubsub::local_channel(16);
            let (repl_tx, _repl_rx) = replication::replica_channel(16);
            let client_id = 1;

            // MULTI
            let mut out1 = Cursor::new(Vec::new());
            let mut w1 = BatchWriter::new(&mut out1);
            let (f1, _) = parser::parse(b"MULTI\r\n").unwrap();
            execute_command_with_repl(f1, &cache, &pubsub_hub, &aof, &repl_hub, &acl_engine, &mut conn_acl, &mut txn_state, client_id, &tx, &repl_tx, &mut w1).await.unwrap();
            w1.flush_all().await.unwrap();
            assert_eq!(out1.into_inner(), b"+OK\r\n");

            // SET {u}k 123
            let mut out2 = Cursor::new(Vec::new());
            let mut w2 = BatchWriter::new(&mut out2);
            let (f2, _) = parser::parse(b"SET {u}k 123\r\n").unwrap();
            execute_command_with_repl(f2, &cache, &pubsub_hub, &aof, &repl_hub, &acl_engine, &mut conn_acl, &mut txn_state, client_id, &tx, &repl_tx, &mut w2).await.unwrap();
            w2.flush_all().await.unwrap();
            assert_eq!(out2.into_inner(), b"+QUEUED\r\n");

            // DISCARD
            let mut out3 = Cursor::new(Vec::new());
            let mut w3 = BatchWriter::new(&mut out3);
            let (f3, _) = parser::parse(b"DISCARD\r\n").unwrap();
            execute_command_with_repl(f3, &cache, &pubsub_hub, &aof, &repl_hub, &acl_engine, &mut conn_acl, &mut txn_state, client_id, &tx, &repl_tx, &mut w3).await.unwrap();
            w3.flush_all().await.unwrap();
            assert_eq!(out3.into_inner(), b"+OK\r\n");

            // State is back to normal
            assert!(!txn_state.in_multi);
            assert_eq!(cache.get(b"{u}k"), None);
        });
    }

    #[test]
    fn test_cluster_moved_redirection_and_local() {
        futures_lite::future::block_on(async {
            let cache = LocalCache::new();
            let pubsub_hub = PubSubHub::new();
            let aof = AofEngine::new("test_cluster_moved.aof", aof::AofSyncMode::No);
            let repl_hub = ReplicationHub::new(1024);
            let acl_engine = AclEngine::new();
            let mut conn_acl = ConnectionAcl::default();
            let mut txn_state = TransactionState::new();
            let (tx, _rx) = pubsub::local_channel(16);
            let (repl_tx, _repl_rx) = replication::replica_channel(16);
            let client_id = 1;

            crate::cluster::set_cluster_enabled(true);
            crate::cluster::with_topology_mut(|top| {
                top.my_port = 7000;
                top.add_slots(&(0..=5460).collect::<Vec<u16>>());
                let p_id = top.meet_peer("127.0.0.1", 7001, 17001);
                for s in 5461..=10922 {
                    top.set_slot_node(s, &p_id);
                }
            });

            let mut node1_key = Vec::new();
            let mut node2_key = Vec::new();
            for i in 0..1000 {
                let candidate = format!("key_{i}").into_bytes();
                let slot = crate::db::key_slot(&candidate);
                if slot <= 5460 && node1_key.is_empty() {
                    node1_key = candidate;
                } else if slot >= 5461 && slot <= 10922 && node2_key.is_empty() {
                    node2_key = candidate;
                }
                if !node1_key.is_empty() && !node2_key.is_empty() {
                    break;
                }
            }

            let slot2 = crate::db::key_slot(&node2_key);

            // 1. Query key on Node 2 -> MUST RETURN -MOVED <slot2> 127.0.0.1:7001\r\n
            let cmd2 = format!("GET {}\r\n", String::from_utf8_lossy(&node2_key));
            let mut out1 = Cursor::new(Vec::new());
            let mut w1 = BatchWriter::new(&mut out1);
            let (f1, _) = parser::parse(cmd2.as_bytes()).unwrap();
            execute_command_with_repl(f1, &cache, &pubsub_hub, &aof, &repl_hub, &acl_engine, &mut conn_acl, &mut txn_state, client_id, &tx, &repl_tx, &mut w1).await.unwrap();
            w1.flush_all().await.unwrap();
            let resp1 = String::from_utf8_lossy(&out1.into_inner()).into_owned();
            assert_eq!(resp1, format!("-MOVED {slot2} 127.0.0.1:7001\r\n"));

            // 2. Query key on Node 1 -> MUST EXECUTE LOCALLY (returns nil for missing key)
            let cmd1 = format!("GET {}\r\n", String::from_utf8_lossy(&node1_key));
            let mut out2 = Cursor::new(Vec::new());
            let mut w2 = BatchWriter::new(&mut out2);
            let (f2, _) = parser::parse(cmd1.as_bytes()).unwrap();
            execute_command_with_repl(f2, &cache, &pubsub_hub, &aof, &repl_hub, &acl_engine, &mut conn_acl, &mut txn_state, client_id, &tx, &repl_tx, &mut w2).await.unwrap();
            w2.flush_all().await.unwrap();
            assert_eq!(out2.into_inner(), b"$-1\r\n");

            crate::cluster::set_cluster_enabled(false);
        });
    }

    #[test]
    fn test_cluster_ask_and_asking_redirection() {
        futures_lite::future::block_on(async {
            let cache = LocalCache::new();
            let pubsub_hub = PubSubHub::new();
            let aof = AofEngine::new("test_cluster_ask.aof", aof::AofSyncMode::No);
            let repl_hub = ReplicationHub::new(1024);
            let acl_engine = AclEngine::new();
            let mut conn_acl = ConnectionAcl::default();
            let mut txn_state = TransactionState::new();
            let (tx, _rx) = pubsub::local_channel(16);
            let (repl_tx, _repl_rx) = replication::replica_channel(16);
            let client_id = 1;

            crate::cluster::set_cluster_enabled(true);
            let node2_id = crate::cluster::with_topology_mut(|top| {
                top.add_slots(&[300]);
                let p_id = top.meet_peer("127.0.0.1", 7001, 17001);
                top.set_slot_migrating(300, &p_id);
                p_id
            });

            let mut key300 = Vec::new();
            for i in 0..10000 {
                let candidate = format!("slot300_key_{i}").into_bytes();
                if crate::db::key_slot(&candidate) == 300 {
                    key300 = candidate;
                    break;
                }
            }
            assert!(!key300.is_empty());

            // 1. Key not present locally in migrating slot 300 -> MUST RETURN -ASK 300 127.0.0.1:7001\r\n
            let cmd_ask = format!("GET {}\r\n", String::from_utf8_lossy(&key300));
            let mut out1 = Cursor::new(Vec::new());
            let mut w1 = BatchWriter::new(&mut out1);
            let (f1, _) = parser::parse(cmd_ask.as_bytes()).unwrap();
            execute_command_with_repl(f1, &cache, &pubsub_hub, &aof, &repl_hub, &acl_engine, &mut conn_acl, &mut txn_state, client_id, &tx, &repl_tx, &mut w1).await.unwrap();
            w1.flush_all().await.unwrap();
            let resp1 = String::from_utf8_lossy(&out1.into_inner()).into_owned();
            assert_eq!(resp1, "-ASK 300 127.0.0.1:7001\r\n");

            // 2. Set key locally -> now present, must execute locally
            cache.set(&key300, b"local_val");
            let mut out2 = Cursor::new(Vec::new());
            let mut w2 = BatchWriter::new(&mut out2);
            let (f2, _) = parser::parse(cmd_ask.as_bytes()).unwrap();
            execute_command_with_repl(f2, &cache, &pubsub_hub, &aof, &repl_hub, &acl_engine, &mut conn_acl, &mut txn_state, client_id, &tx, &repl_tx, &mut w2).await.unwrap();
            w2.flush_all().await.unwrap();
            assert_eq!(out2.into_inner(), b"$9\r\nlocal_val\r\n");

            // 3. Test ASKING on importing slot 400
            let mut key400 = Vec::new();
            for i in 0..10000 {
                let candidate = format!("slot400_key_{i}").into_bytes();
                if crate::db::key_slot(&candidate) == 400 {
                    key400 = candidate;
                    break;
                }
            }
            crate::cluster::with_topology_mut(|top| {
                top.set_slot_node(400, &node2_id);
                top.set_slot_importing(400, &node2_id);
            });

            // 3a. Without ASKING -> returns -MOVED
            let cmd400 = format!("SET {} 999\r\n", String::from_utf8_lossy(&key400));
            let mut out3 = Cursor::new(Vec::new());
            let mut w3 = BatchWriter::new(&mut out3);
            let (f3, _) = parser::parse(cmd400.as_bytes()).unwrap();
            execute_command_with_repl(f3, &cache, &pubsub_hub, &aof, &repl_hub, &acl_engine, &mut conn_acl, &mut txn_state, client_id, &tx, &repl_tx, &mut w3).await.unwrap();
            w3.flush_all().await.unwrap();
            let resp3 = String::from_utf8_lossy(&out3.into_inner()).into_owned();
            assert_eq!(resp3, "-MOVED 400 127.0.0.1:7001\r\n");

            // 3b. Send ASKING first
            let mut out_asking = Cursor::new(Vec::new());
            let mut w_asking = BatchWriter::new(&mut out_asking);
            let (f_asking, _) = parser::parse(b"ASKING\r\n").unwrap();
            execute_command_with_repl(f_asking, &cache, &pubsub_hub, &aof, &repl_hub, &acl_engine, &mut conn_acl, &mut txn_state, client_id, &tx, &repl_tx, &mut w_asking).await.unwrap();
            w_asking.flush_all().await.unwrap();
            assert_eq!(out_asking.into_inner(), b"+OK\r\n");
            assert!(txn_state.asking);

            // 3c. Send command on importing slot -> MUST EXECUTE LOCALLY!
            let mut out4 = Cursor::new(Vec::new());
            let mut w4 = BatchWriter::new(&mut out4);
            let (f4, _) = parser::parse(cmd400.as_bytes()).unwrap();
            execute_command_with_repl(f4, &cache, &pubsub_hub, &aof, &repl_hub, &acl_engine, &mut conn_acl, &mut txn_state, client_id, &tx, &repl_tx, &mut w4).await.unwrap();
            w4.flush_all().await.unwrap();
            assert_eq!(out4.into_inner(), b"+OK\r\n");
            assert!(!txn_state.asking);

            crate::cluster::set_cluster_enabled(false);
        });
    }
}
