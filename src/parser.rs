//! Zero-Copy RESP (Redis Serialization Protocol) Parser for VelocityDB.
//!
//! # Architecture & Performance Guarantees:
//! - **Zero Heap Allocations**: All parsed frames return `&'a [u8]` slices directly referencing
//!   the caller's buffer. No `String`, `Vec`, or dynamic memory allocation.
//! - **SIMD Acceleration**: Leverages the `memchr` crate (`memmem::find` and `memchr2`) to scan
//!   delimiter boundaries (`\r\n`, spaces, tabs) at vector register speeds.
//! - **Partial Buffer Support**: Accurately returns `ParseError::Incomplete` on fragmented I/O frames
//!   so the thread-per-core reactor can wait for more bytes from `io_uring`.
//! - **Inline & Array Support**: Uniformly handles standard RESP arrays of bulk strings
//!   and legacy/telnet inline commands.

use memchr::{memchr, memchr2, memmem};

/// Errors encountered during zero-copy RESP parsing.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum ParseError {
    /// Frame is incomplete in the provided buffer; more data needed from socket.
    Incomplete,
    /// Frame structure or trailing delimiter violates RESP specification.
    InvalidFormat,
    /// Integer length or count cannot be parsed or overflows `i64`.
    InvalidInteger,
    /// Length or element count is negative (excluding allowed null representation `-1`).
    NegativeLength,
    /// Frame prefix byte is unrecognized.
    UnknownPrefix(u8),
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Incomplete => write!(f, "Incomplete RESP frame in buffer"),
            Self::InvalidFormat => write!(f, "Invalid RESP framing format"),
            Self::InvalidInteger => write!(f, "Invalid integer format or overflow"),
            Self::NegativeLength => write!(f, "Unexpected negative length"),
            Self::UnknownPrefix(b) => write!(f, "Unknown RESP prefix byte: {b:#04x}"),
        }
    }
}

impl std::error::Error for ParseError {}

/// A zero-copy RESP frame referencing the underlying buffer slice `'a`.
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum RespFrame<'a> {
    /// Simple string `+<data>\r\n`
    SimpleString(&'a [u8]),
    /// Error message `-<data>\r\n`
    Error(&'a [u8]),
    /// Signed 64-bit integer `:<num>\r\n`
    Integer(i64),
    /// Bulk string `$<len>\r\n<data>\r\n`
    BulkString(&'a [u8]),
    /// Null representation `$-1\r\n` or `*-1\r\n`
    Null,
    /// Array `*<count>\r\n<elements...>`
    Array(ArrayIter<'a>),
    /// Inline command `<token1> <token2> ...\r\n`
    Inline(InlineIter<'a>),
}

impl<'a> RespFrame<'a> {
    /// Returns an iterator over command arguments as `&'a [u8]` slices without any heap allocation.
    #[inline]
    pub fn args(self) -> CommandArgs<'a> {
        match self {
            Self::Array(iter) => CommandArgs::Array(iter),
            Self::Inline(iter) => CommandArgs::Inline(iter),
            Self::BulkString(s) | Self::SimpleString(s) => CommandArgs::Single(s),
            _ => CommandArgs::Empty,
        }
    }

    /// Returns the underlying byte slice for bulk strings, simple strings, or errors.
    #[inline]
    pub fn as_bytes(&self) -> Option<&'a [u8]> {
        match self {
            Self::BulkString(s) | Self::SimpleString(s) | Self::Error(s) => Some(*s),
            _ => None,
        }
    }

    /// Returns the integer value if the frame is an `Integer`.
    #[inline]
    pub fn as_integer(&self) -> Option<i64> {
        match self {
            Self::Integer(val) => Some(*val),
            _ => None,
        }
    }

    /// Returns `true` if this frame is a `Null` bulk string or null array.
    #[inline]
    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }
}

/// Zero-allocation lazy iterator over elements of a RESP Array.
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct ArrayIter<'a> {
    data: &'a [u8],
    remaining: usize,
}

impl<'a> ArrayIter<'a> {
    /// Creates a new `ArrayIter` over the serialized element bytes.
    #[inline]
    pub const fn new(data: &'a [u8], count: usize) -> Self {
        Self { data, remaining: count }
    }

    /// Number of remaining elements in the array.
    #[inline]
    pub const fn remaining(&self) -> usize {
        self.remaining
    }

    /// Returns `true` if there are no more elements to yield.
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.remaining == 0
    }
}

impl<'a> Iterator for ArrayIter<'a> {
    type Item = RespFrame<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 || self.data.is_empty() {
            return None;
        }

        // Elements were pre-validated during `parse_array`, so this is guaranteed to succeed.
        match parse(self.data) {
            Ok((frame, consumed)) => {
                self.data = &self.data[consumed..];
                self.remaining -= 1;
                Some(frame)
            }
            Err(_) => {
                self.remaining = 0;
                self.data = &[];
                None
            }
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl<'a> ExactSizeIterator for ArrayIter<'a> {}

/// Zero-allocation lazy iterator over whitespace-delimited tokens in an Inline command.
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct InlineIter<'a> {
    data: &'a [u8],
}

impl<'a> InlineIter<'a> {
    /// Creates a new `InlineIter` from an inline command line slice.
    #[inline]
    pub const fn new(data: &'a [u8]) -> Self {
        Self { data }
    }
}

impl<'a> Iterator for InlineIter<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<Self::Item> {
        // Skip leading whitespace (spaces and tabs)
        let mut start = 0;
        while start < self.data.len() && (self.data[start] == b' ' || self.data[start] == b'\t') {
            start += 1;
        }

        if start >= self.data.len() {
            self.data = &[];
            return None;
        }

        // Support quoted arguments (e.g. "my key" or 'my key')
        if self.data[start] == b'"' || self.data[start] == b'\'' {
            let quote = self.data[start];
            let content_start = start + 1;
            if let Some(quote_idx) = memchr(quote, &self.data[content_start..]) {
                let token_end = content_start + quote_idx;
                let token = &self.data[content_start..token_end];
                self.data = &self.data[token_end + 1..];
                return Some(token);
            }
        }

        // Unquoted argument: locate next whitespace boundary via SIMD memchr2
        let token_slice = &self.data[start..];
        let token_len = memchr2(b' ', b'\t', token_slice).unwrap_or(token_slice.len());

        let token = &token_slice[..token_len];
        self.data = &token_slice[token_len..];
        Some(token)
    }
}

/// Zero-allocation unified iterator yielding command arguments as `&'a [u8]`.
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum CommandArgs<'a> {
    /// Arguments from a RESP Array of Bulk Strings.
    Array(ArrayIter<'a>),
    /// Arguments from an Inline command line.
    Inline(InlineIter<'a>),
    /// Single argument frame.
    Single(&'a [u8]),
    /// Empty argument stream.
    Empty,
}

impl<'a> Iterator for CommandArgs<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Array(iter) => {
                while let Some(frame) = iter.next() {
                    match frame {
                        RespFrame::BulkString(s) | RespFrame::SimpleString(s) => return Some(s),
                        _ => {}
                    }
                }
                None
            }
            Self::Inline(iter) => iter.next(),
            Self::Single(s) => {
                let res = *s;
                *self = Self::Empty;
                Some(res)
            }
            Self::Empty => None,
        }
    }
}

/// Parses a zero-copy RESP frame from `src`.
///
/// On success, returns `Ok((RespFrame, bytes_consumed))`.
/// If the buffer does not yet contain a complete frame, returns `Err(ParseError::Incomplete)`.
pub fn parse(src: &[u8]) -> Result<(RespFrame<'_>, usize), ParseError> {
    if src.is_empty() {
        return Err(ParseError::Incomplete);
    }

    match src[0] {
        b'+' => parse_simple_string(src),
        b'-' => parse_error(src),
        b':' => parse_integer(src),
        b'$' => parse_bulk_string(src),
        b'*' => parse_array(src),
        _ => parse_inline(src),
    }
}

/// Parses a Simple String: `+<content>\r\n`
#[inline]
fn parse_simple_string(src: &[u8]) -> Result<(RespFrame<'_>, usize), ParseError> {
    let line_content = &src[1..];
    let crlf = memmem::find(line_content, b"\r\n").ok_or(ParseError::Incomplete)?;
    let slice = &line_content[..crlf];
    let consumed = 1 + crlf + 2;
    Ok((RespFrame::SimpleString(slice), consumed))
}

/// Parses an Error: `-<content>\r\n`
#[inline]
fn parse_error(src: &[u8]) -> Result<(RespFrame<'_>, usize), ParseError> {
    let line_content = &src[1..];
    let crlf = memmem::find(line_content, b"\r\n").ok_or(ParseError::Incomplete)?;
    let slice = &line_content[..crlf];
    let consumed = 1 + crlf + 2;
    Ok((RespFrame::Error(slice), consumed))
}

/// Parses an Integer: `:<signed 64-bit int>\r\n`
#[inline]
fn parse_integer(src: &[u8]) -> Result<(RespFrame<'_>, usize), ParseError> {
    let line_content = &src[1..];
    let crlf = memmem::find(line_content, b"\r\n").ok_or(ParseError::Incomplete)?;
    let int_slice = &line_content[..crlf];
    let val = parse_i64(int_slice)?;
    let consumed = 1 + crlf + 2;
    Ok((RespFrame::Integer(val), consumed))
}

/// Parses a Bulk String: `$<len>\r\n<data>\r\n` or Null: `$-1\r\n`
#[inline]
fn parse_bulk_string(src: &[u8]) -> Result<(RespFrame<'_>, usize), ParseError> {
    let line_content = &src[1..];
    let crlf = memmem::find(line_content, b"\r\n").ok_or(ParseError::Incomplete)?;
    let len_slice = &line_content[..crlf];
    let len = parse_i64(len_slice)?;

    let header_len = 1 + crlf + 2;

    if len == -1 {
        return Ok((RespFrame::Null, header_len));
    }

    if len < -1 {
        return Err(ParseError::NegativeLength);
    }

    let ulen = len as usize;
    let data_start = header_len;
    let data_end = match data_start.checked_add(ulen) {
        Some(end) => end,
        None => return Err(ParseError::InvalidInteger),
    };
    let total_needed = match data_end.checked_add(2) {
        Some(total) => total,
        None => return Err(ParseError::InvalidInteger),
    };

    if src.len() < total_needed {
        return Err(ParseError::Incomplete);
    }

    // Verify trailing \r\n boundary directly after data payload
    if &src[data_end..total_needed] != b"\r\n" {
        return Err(ParseError::InvalidFormat);
    }

    let data = &src[data_start..data_end];
    Ok((RespFrame::BulkString(data), total_needed))
}

/// Parses a RESP Array: `*<count>\r\n<elements...>` or Null: `*-1\r\n`
#[inline]
fn parse_array(src: &[u8]) -> Result<(RespFrame<'_>, usize), ParseError> {
    let line_content = &src[1..];
    let crlf = memmem::find(line_content, b"\r\n").ok_or(ParseError::Incomplete)?;
    let count_slice = &line_content[..crlf];
    let count_i64 = parse_i64(count_slice)?;

    let header_len = 1 + crlf + 2;

    if count_i64 == -1 {
        return Ok((RespFrame::Null, header_len));
    }

    if count_i64 < -1 {
        return Err(ParseError::NegativeLength);
    }

    let count = count_i64 as usize;
    if count == 0 {
        return Ok((RespFrame::Array(ArrayIter::new(&[], 0)), header_len));
    }

    // Validate that all `count` child elements are present and well-formed
    let mut offset = header_len;
    for _ in 0..count {
        if offset >= src.len() {
            return Err(ParseError::Incomplete);
        }
        let (_, elem_consumed) = parse(&src[offset..])?;
        offset += elem_consumed;
    }

    let array_payload = &src[header_len..offset];
    Ok((
        RespFrame::Array(ArrayIter::new(array_payload, count)),
        offset,
    ))
}

/// Parses an Inline Command line terminated by `\r\n` or `\n`.
#[inline]
fn parse_inline(src: &[u8]) -> Result<(RespFrame<'_>, usize), ParseError> {
    let (line_len, newline_len) = match memmem::find(src, b"\r\n") {
        Some(pos) => (pos, 2),
        None => match memchr(b'\n', src) {
            Some(pos) => {
                if pos > 0 && src[pos - 1] == b'\r' {
                    (pos - 1, 2)
                } else {
                    (pos, 1)
                }
            }
            None => return Err(ParseError::Incomplete),
        },
    };

    let line = &src[..line_len];
    let consumed = line_len + newline_len;
    Ok((RespFrame::Inline(InlineIter::new(line)), consumed))
}

/// Fast, allocation-free ASCII integer parser supporting optional +/- signs.
#[inline]
pub fn parse_i64(src: &[u8]) -> Result<i64, ParseError> {
    if src.is_empty() {
        return Err(ParseError::InvalidInteger);
    }

    let (is_negative, digits) = match src[0] {
        b'-' => (true, &src[1..]),
        b'+' => (false, &src[1..]),
        _ => (false, src),
    };

    if digits.is_empty() {
        return Err(ParseError::InvalidInteger);
    }

    let mut result: i64 = 0;
    for &byte in digits {
        if !byte.is_ascii_digit() {
            return Err(ParseError::InvalidInteger);
        }
        let digit = (byte - b'0') as i64;
        result = result
            .checked_mul(10)
            .and_then(|r| {
                if is_negative {
                    r.checked_sub(digit)
                } else {
                    r.checked_add(digit)
                }
            })
            .ok_or(ParseError::InvalidInteger)?;
    }

    Ok(result)
}

/// Zero-allocation lazy iterator for consuming a pipelined batch of RESP frames.
#[derive(Debug, Clone)]
pub struct PipelineIter<'a> {
    data: &'a [u8],
    consumed: usize,
}

impl<'a> PipelineIter<'a> {
    /// Creates a new `PipelineIter` over `data`.
    #[inline]
    pub const fn new(data: &'a [u8]) -> Self {
        Self { data, consumed: 0 }
    }

    /// Total bytes consumed by successfully parsed frames in this pipeline batch.
    #[inline]
    pub const fn consumed(&self) -> usize {
        self.consumed
    }

    /// Slice containing unparsed or partial trailing bytes.
    #[inline]
    pub const fn remaining(&self) -> &'a [u8] {
        self.data
    }
}

impl<'a> Iterator for PipelineIter<'a> {
    type Item = Result<RespFrame<'a>, ParseError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.data.is_empty() {
            return None;
        }

        match parse(self.data) {
            Ok((frame, len)) => {
                self.data = &self.data[len..];
                self.consumed += len;
                Some(Ok(frame))
            }
            Err(e) => {
                // Terminate iteration on incomplete frame or syntax error
                self.data = &[];
                Some(Err(e))
            }
        }
    }
}

/// Creates a zero-allocation `PipelineIter` over `src` for aggressive batch processing.
#[inline]
pub fn parse_pipeline(src: &[u8]) -> PipelineIter<'_> {
    PipelineIter::new(src)
}

/// Strongly-typed zero-copy parsed command referencing the underlying buffer slices.
#[derive(Debug, PartialEq, Clone)]
pub enum Command<'a> {
    /// PING [message]
    Ping(Option<&'a [u8]>),
    /// SET key value [EX seconds | PX millis]
    Set {
        key: &'a [u8],
        val: &'a [u8],
        ttl_ms: Option<u64>,
    },
    /// GET key
    Get(&'a [u8]),
    /// DEL key
    Del(&'a [u8]),
    /// SUBSCRIBE channel [channel ...]
    Subscribe(Vec<&'a [u8]>),
    /// UNSUBSCRIBE [channel ...]
    Unsubscribe(Vec<&'a [u8]>),
    /// PUBLISH channel message
    Publish {
        channel: &'a [u8],
        message: &'a [u8],
    },
    /// HSET key field value
    HSet {
        key: &'a [u8],
        field: &'a [u8],
        val: &'a [u8],
    },
    /// HGET key field
    HGet {
        key: &'a [u8],
        field: &'a [u8],
    },
    /// HDEL key field
    HDel {
        key: &'a [u8],
        field: &'a [u8],
    },
    /// LPUSH key value
    LPush {
        key: &'a [u8],
        val: &'a [u8],
    },
    /// RPUSH key value
    RPush {
        key: &'a [u8],
        val: &'a [u8],
    },
    /// LPOP key
    LPop(&'a [u8]),
    /// RPOP key
    RPop(&'a [u8]),
    /// ZADD key score member
    ZAdd {
        key: &'a [u8],
        score: f64,
        member: &'a [u8],
    },
    /// ZSCORE key member
    ZScore {
        key: &'a [u8],
        member: &'a [u8],
    },
    /// COMMAND discovery
    Command,
    /// PSYNC <replid> <offset>
    PSync {
        replid: &'a [u8],
        offset: i64,
    },
    /// SYNC full resynchronization
    Sync,
    /// REPLCONF <subcommands...>
    ReplConf(Vec<&'a [u8]>),
    /// ROLE discovery
    Role,
    /// INFO [section]
    Info(Option<&'a [u8]>),
    /// MULTI starts transaction
    Multi,
    /// EXEC commits transaction
    Exec,
    /// DISCARD aborts transaction
    Discard,
    /// SETBIT key offset value
    SetBit {
        key: &'a [u8],
        offset: usize,
        value: u8,
    },
    /// GETBIT key offset
    GetBit {
        key: &'a [u8],
        offset: usize,
    },
    /// BITCOUNT key [start end]
    BitCount {
        key: &'a [u8],
        start: Option<isize>,
        end: Option<isize>,
    },
    /// XADD key [ID] field value ...
    XAdd {
        key: &'a [u8],
        id: Option<&'a [u8]>,
        fields: Vec<(&'a [u8], &'a [u8])>,
    },
    /// XREAD [COUNT count] STREAMS key ... id ...
    XRead {
        streams: Vec<(&'a [u8], &'a [u8])>,
        count: Option<usize>,
    },
    /// XLEN key
    XLen(&'a [u8]),
    /// CLUSTER <subcommand> [args...]
    Cluster(Vec<&'a [u8]>),
    /// ASKING sets connection flag for cluster migration
    Asking,
    /// Unrecognized command
    Unknown(&'a [u8]),
    /// Command has wrong number of arguments
    WrongArgs(&'static str),
}

/// Parses a zero-copy RESP frame into a strongly typed `Command<'a>`.
pub fn parse_command<'a>(frame: RespFrame<'a>) -> Command<'a> {
    let mut args = frame.args();
    let cmd = match args.next() {
        Some(c) => c,
        None => return Command::Unknown(b""),
    };

    if cmd.eq_ignore_ascii_case(b"PING") {
        Command::Ping(args.next())
    } else if cmd.eq_ignore_ascii_case(b"SET") {
        let key = match args.next() {
            Some(k) => k,
            None => return Command::WrongArgs("set"),
        };
        let val = match args.next() {
            Some(v) => v,
            None => return Command::WrongArgs("set"),
        };
        let mut ttl_ms = None;
        if let Some(opt) = args.next() {
            if opt.eq_ignore_ascii_case(b"EX") {
                if let Some(sec_str) = args.next() {
                    if let Ok(sec) = parse_i64(sec_str) {
                        if sec > 0 {
                            ttl_ms = Some((sec as u64) * 1000);
                        }
                    }
                }
            } else if opt.eq_ignore_ascii_case(b"PX") {
                if let Some(ms_str) = args.next() {
                    if let Ok(ms) = parse_i64(ms_str) {
                        if ms > 0 {
                            ttl_ms = Some(ms as u64);
                        }
                    }
                }
            }
        }
        Command::Set { key, val, ttl_ms }
    } else if cmd.eq_ignore_ascii_case(b"GET") {
        match args.next() {
            Some(k) => Command::Get(k),
            None => Command::WrongArgs("get"),
        }
    } else if cmd.eq_ignore_ascii_case(b"DEL") {
        match args.next() {
            Some(k) => Command::Del(k),
            None => Command::WrongArgs("del"),
        }
    } else if cmd.eq_ignore_ascii_case(b"SUBSCRIBE") {
        let mut channels = Vec::new();
        for ch in args {
            channels.push(ch);
        }
        if channels.is_empty() {
            Command::WrongArgs("subscribe")
        } else {
            Command::Subscribe(channels)
        }
    } else if cmd.eq_ignore_ascii_case(b"UNSUBSCRIBE") {
        let mut channels = Vec::new();
        for ch in args {
            channels.push(ch);
        }
        Command::Unsubscribe(channels)
    } else if cmd.eq_ignore_ascii_case(b"PUBLISH") {
        let channel = match args.next() {
            Some(ch) => ch,
            None => return Command::WrongArgs("publish"),
        };
        let message = match args.next() {
            Some(msg) => msg,
            None => return Command::WrongArgs("publish"),
        };
        Command::Publish { channel, message }
    } else if cmd.eq_ignore_ascii_case(b"HSET") {
        let key = match args.next() {
            Some(k) => k,
            None => return Command::WrongArgs("hset"),
        };
        let field = match args.next() {
            Some(f) => f,
            None => return Command::WrongArgs("hset"),
        };
        let val = match args.next() {
            Some(v) => v,
            None => return Command::WrongArgs("hset"),
        };
        Command::HSet { key, field, val }
    } else if cmd.eq_ignore_ascii_case(b"HGET") {
        let key = match args.next() {
            Some(k) => k,
            None => return Command::WrongArgs("hget"),
        };
        let field = match args.next() {
            Some(f) => f,
            None => return Command::WrongArgs("hget"),
        };
        Command::HGet { key, field }
    } else if cmd.eq_ignore_ascii_case(b"HDEL") {
        let key = match args.next() {
            Some(k) => k,
            None => return Command::WrongArgs("hdel"),
        };
        let field = match args.next() {
            Some(f) => f,
            None => return Command::WrongArgs("hdel"),
        };
        Command::HDel { key, field }
    } else if cmd.eq_ignore_ascii_case(b"LPUSH") {
        let key = match args.next() {
            Some(k) => k,
            None => return Command::WrongArgs("lpush"),
        };
        let val = match args.next() {
            Some(v) => v,
            None => return Command::WrongArgs("lpush"),
        };
        Command::LPush { key, val }
    } else if cmd.eq_ignore_ascii_case(b"RPUSH") {
        let key = match args.next() {
            Some(k) => k,
            None => return Command::WrongArgs("rpush"),
        };
        let val = match args.next() {
            Some(v) => v,
            None => return Command::WrongArgs("rpush"),
        };
        Command::RPush { key, val }
    } else if cmd.eq_ignore_ascii_case(b"LPOP") {
        match args.next() {
            Some(k) => Command::LPop(k),
            None => Command::WrongArgs("lpop"),
        }
    } else if cmd.eq_ignore_ascii_case(b"RPOP") {
        match args.next() {
            Some(k) => Command::RPop(k),
            None => Command::WrongArgs("rpop"),
        }
    } else if cmd.eq_ignore_ascii_case(b"ZADD") {
        let key = match args.next() {
            Some(k) => k,
            None => return Command::WrongArgs("zadd"),
        };
        let score_bytes = match args.next() {
            Some(s) => s,
            None => return Command::WrongArgs("zadd"),
        };
        let score_str = match std::str::from_utf8(score_bytes) {
            Ok(s) => s,
            Err(_) => return Command::WrongArgs("zadd"),
        };
        let score: f64 = match score_str.parse() {
            Ok(s) => s,
            Err(_) => return Command::WrongArgs("zadd"),
        };
        let member = match args.next() {
            Some(m) => m,
            None => return Command::WrongArgs("zadd"),
        };
        Command::ZAdd { key, score, member }
    } else if cmd.eq_ignore_ascii_case(b"ZSCORE") {
        let key = match args.next() {
            Some(k) => k,
            None => return Command::WrongArgs("zscore"),
        };
        let member = match args.next() {
            Some(m) => m,
            None => return Command::WrongArgs("zscore"),
        };
        Command::ZScore { key, member }
    } else if cmd.eq_ignore_ascii_case(b"COMMAND") {
        Command::Command
    } else if cmd.eq_ignore_ascii_case(b"PSYNC") {
        let replid = match args.next() {
            Some(r) => r,
            None => return Command::WrongArgs("psync"),
        };
        let offset = match args.next() {
            Some(o) => parse_i64(o).unwrap_or(-1),
            None => -1,
        };
        Command::PSync { replid, offset }
    } else if cmd.eq_ignore_ascii_case(b"SYNC") {
        Command::Sync
    } else if cmd.eq_ignore_ascii_case(b"REPLCONF") {
        let mut subargs = Vec::new();
        for a in args {
            subargs.push(a);
        }
        Command::ReplConf(subargs)
    } else if cmd.eq_ignore_ascii_case(b"ROLE") {
        Command::Role
    } else if cmd.eq_ignore_ascii_case(b"INFO") {
        Command::Info(args.next())
    } else if cmd.eq_ignore_ascii_case(b"MULTI") {
        Command::Multi
    } else if cmd.eq_ignore_ascii_case(b"EXEC") {
        Command::Exec
    } else if cmd.eq_ignore_ascii_case(b"DISCARD") {
        Command::Discard
    } else if cmd.eq_ignore_ascii_case(b"SETBIT") {
        let key = match args.next() {
            Some(k) => k,
            None => return Command::WrongArgs("setbit"),
        };
        let off_b = match args.next() {
            Some(o) => o,
            None => return Command::WrongArgs("setbit"),
        };
        let val_b = match args.next() {
            Some(v) => v,
            None => return Command::WrongArgs("setbit"),
        };
        let offset = match parse_i64(off_b) {
            Ok(o) if o >= 0 => o as usize,
            _ => return Command::WrongArgs("setbit"),
        };
        let val = match parse_i64(val_b) {
            Ok(v) if v == 0 || v == 1 => v as u8,
            _ => return Command::WrongArgs("setbit"),
        };
        Command::SetBit { key, offset, value: val }
    } else if cmd.eq_ignore_ascii_case(b"GETBIT") {
        let key = match args.next() {
            Some(k) => k,
            None => return Command::WrongArgs("getbit"),
        };
        let off_b = match args.next() {
            Some(o) => o,
            None => return Command::WrongArgs("getbit"),
        };
        let offset = match parse_i64(off_b) {
            Ok(o) if o >= 0 => o as usize,
            _ => return Command::WrongArgs("getbit"),
        };
        Command::GetBit { key, offset }
    } else if cmd.eq_ignore_ascii_case(b"BITCOUNT") {
        let key = match args.next() {
            Some(k) => k,
            None => return Command::WrongArgs("bitcount"),
        };
        let start = args.next().and_then(|b| parse_i64(b).ok()).map(|v| v as isize);
        let end = args.next().and_then(|b| parse_i64(b).ok()).map(|v| v as isize);
        Command::BitCount { key, start, end }
    } else if cmd.eq_ignore_ascii_case(b"XADD") {
        let key = match args.next() {
            Some(k) => k,
            None => return Command::WrongArgs("xadd"),
        };
        let id_or_field = match args.next() {
            Some(i) => i,
            None => return Command::WrongArgs("xadd"),
        };
        let (id, first_field) = if id_or_field == b"*" || id_or_field.contains(&b'-') {
            (Some(id_or_field), args.next())
        } else {
            (None, Some(id_or_field))
        };
        let mut fields = Vec::new();
        if let Some(f1) = first_field {
            let v1 = match args.next() {
                Some(v) => v,
                None => return Command::WrongArgs("xadd"),
            };
            fields.push((f1, v1));
            while let Some(f) = args.next() {
                let v = match args.next() {
                    Some(val) => val,
                    None => return Command::WrongArgs("xadd"),
                };
                fields.push((f, v));
            }
        } else {
            return Command::WrongArgs("xadd");
        }
        Command::XAdd { key, id, fields }
    } else if cmd.eq_ignore_ascii_case(b"XREAD") {
        let mut count = None;
        let mut streams = Vec::new();
        let mut in_streams = false;
        let mut raw_stream_args = Vec::new();

        while let Some(arg) = args.next() {
            if in_streams {
                raw_stream_args.push(arg);
            } else if arg.eq_ignore_ascii_case(b"COUNT") {
                if let Some(c_b) = args.next() {
                    if let Ok(c) = parse_i64(c_b) {
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
            return Command::WrongArgs("xread");
        }
        let half = raw_stream_args.len() / 2;
        for i in 0..half {
            streams.push((raw_stream_args[i], raw_stream_args[half + i]));
        }
        Command::XRead { streams, count }
    } else if cmd.eq_ignore_ascii_case(b"XLEN") {
        match args.next() {
            Some(k) => Command::XLen(k),
            None => Command::WrongArgs("xlen"),
        }
    } else if cmd.eq_ignore_ascii_case(b"CLUSTER") {
        let subargs: Vec<&'a [u8]> = args.collect();
        Command::Cluster(subargs)
    } else if cmd.eq_ignore_ascii_case(b"ASKING") {
        Command::Asking
    } else {
        Command::Unknown(cmd)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_string() {
        let input = b"+OK\r\n";
        let (frame, consumed) = parse(input).expect("parse simple string");
        assert_eq!(consumed, 5);
        assert_eq!(frame, RespFrame::SimpleString(b"OK"));
        assert_eq!(frame.as_bytes(), Some(&b"OK"[..]));
    }

    #[test]
    fn test_parse_simple_string_empty() {
        let input = b"+\r\n";
        let (frame, consumed) = parse(input).expect("parse empty simple string");
        assert_eq!(consumed, 3);
        assert_eq!(frame, RespFrame::SimpleString(b""));
    }

    #[test]
    fn test_parse_error() {
        let input = b"-ERR unknown command 'foobar'\r\n";
        let (frame, consumed) = parse(input).expect("parse error");
        assert_eq!(consumed, input.len());
        assert_eq!(frame, RespFrame::Error(b"ERR unknown command 'foobar'"));
    }

    #[test]
    fn test_parse_integer() {
        let input = b":1000\r\n";
        let (frame, consumed) = parse(input).expect("parse integer");
        assert_eq!(consumed, 7);
        assert_eq!(frame, RespFrame::Integer(1000));
        assert_eq!(frame.as_integer(), Some(1000));
    }

    #[test]
    fn test_parse_integer_negative() {
        let input = b":-42\r\n";
        let (frame, consumed) = parse(input).expect("parse negative integer");
        assert_eq!(consumed, 6);
        assert_eq!(frame, RespFrame::Integer(-42));
        assert_eq!(frame.as_integer(), Some(-42));
    }

    #[test]
    fn test_parse_integer_zero() {
        let input = b":0\r\n";
        let (frame, consumed) = parse(input).expect("parse zero integer");
        assert_eq!(consumed, 4);
        assert_eq!(frame, RespFrame::Integer(0));
    }

    #[test]
    fn test_parse_integer_overflow() {
        let input = b":9999999999999999999999999999\r\n";
        assert_eq!(parse(input), Err(ParseError::InvalidInteger));
    }

    #[test]
    fn test_parse_bulk_string() {
        let input = b"$6\r\nfoobar\r\n";
        let (frame, consumed) = parse(input).expect("parse bulk string");
        assert_eq!(consumed, 12);
        assert_eq!(frame, RespFrame::BulkString(b"foobar"));
        assert_eq!(frame.as_bytes(), Some(&b"foobar"[..]));
    }

    #[test]
    fn test_parse_bulk_string_empty() {
        let input = b"$0\r\n\r\n";
        let (frame, consumed) = parse(input).expect("parse empty bulk string");
        assert_eq!(consumed, 6);
        assert_eq!(frame, RespFrame::BulkString(b""));
    }

    #[test]
    fn test_parse_bulk_string_null() {
        let input = b"$-1\r\n";
        let (frame, consumed) = parse(input).expect("parse null bulk string");
        assert_eq!(consumed, 5);
        assert_eq!(frame, RespFrame::Null);
        assert!(frame.is_null());
    }

    #[test]
    fn test_parse_bulk_string_binary_safe() {
        // Bulk string containing binary data with embedded \r\n
        let input = b"$6\r\nAB\r\nCD\r\n";
        let (frame, consumed) = parse(input).expect("parse binary bulk string");
        assert_eq!(consumed, 12);
        assert_eq!(frame, RespFrame::BulkString(b"AB\r\nCD"));
    }

    #[test]
    fn test_parse_bulk_string_invalid_trailer() {
        let input = b"$3\r\nfooXX";
        assert_eq!(parse(input), Err(ParseError::InvalidFormat));
    }

    #[test]
    fn test_parse_array_empty() {
        let input = b"*0\r\n";
        let (frame, consumed) = parse(input).expect("parse empty array");
        assert_eq!(consumed, 4);
        if let RespFrame::Array(iter) = frame {
            assert_eq!(iter.remaining(), 0);
            assert!(iter.is_empty());
        } else {
            panic!("Expected RespFrame::Array");
        }
    }

    #[test]
    fn test_parse_array_null() {
        let input = b"*-1\r\n";
        let (frame, consumed) = parse(input).expect("parse null array");
        assert_eq!(consumed, 5);
        assert_eq!(frame, RespFrame::Null);
        assert!(frame.is_null());
    }

    #[test]
    fn test_parse_array_of_bulk_strings() {
        let input = b"*2\r\n$3\r\nfoo\r\n$3\r\nbar\r\n";
        let (frame, consumed) = parse(input).expect("parse array");
        assert_eq!(consumed, input.len());

        if let RespFrame::Array(mut iter) = frame {
            assert_eq!(iter.remaining(), 2);
            assert_eq!(iter.next(), Some(RespFrame::BulkString(b"foo")));
            assert_eq!(iter.next(), Some(RespFrame::BulkString(b"bar")));
            assert_eq!(iter.next(), None);
        } else {
            panic!("Expected RespFrame::Array");
        }
    }

    #[test]
    fn test_parse_array_nested() {
        let input = b"*2\r\n*1\r\n$4\r\nPING\r\n$4\r\nPONG\r\n";
        let (frame, consumed) = parse(input).expect("parse nested array");
        assert_eq!(consumed, input.len());

        if let RespFrame::Array(mut iter) = frame {
            assert_eq!(iter.remaining(), 2);
            let first = iter.next().expect("first element");
            if let RespFrame::Array(mut sub_iter) = first {
                assert_eq!(sub_iter.next(), Some(RespFrame::BulkString(b"PING")));
                assert_eq!(sub_iter.next(), None);
            } else {
                panic!("Expected nested RespFrame::Array");
            }
            assert_eq!(iter.next(), Some(RespFrame::BulkString(b"PONG")));
            assert_eq!(iter.next(), None);
        } else {
            panic!("Expected RespFrame::Array");
        }
    }

    #[test]
    fn test_parse_array_incomplete() {
        // Missing second element
        let input = b"*2\r\n$3\r\nfoo\r\n";
        assert_eq!(parse(input), Err(ParseError::Incomplete));

        // Incomplete element length
        let input2 = b"*2\r\n$3\r\nfoo\r\n$3\r\nba";
        assert_eq!(parse(input2), Err(ParseError::Incomplete));
    }

    #[test]
    fn test_parse_inline_simple() {
        let input = b"PING\r\n";
        let (frame, consumed) = parse(input).expect("parse inline PING");
        assert_eq!(consumed, 6);

        if let RespFrame::Inline(mut iter) = frame {
            assert_eq!(iter.next(), Some(&b"PING"[..]));
            assert_eq!(iter.next(), None);
        } else {
            panic!("Expected RespFrame::Inline");
        }
    }

    #[test]
    fn test_parse_inline_with_multiple_args_and_whitespace() {
        let input = b"SET   mykey   myval   \r\n";
        let (frame, consumed) = parse(input).expect("parse inline SET");
        assert_eq!(consumed, input.len());

        let mut args = frame.args();
        assert_eq!(args.next(), Some(&b"SET"[..]));
        assert_eq!(args.next(), Some(&b"mykey"[..]));
        assert_eq!(args.next(), Some(&b"myval"[..]));
        assert_eq!(args.next(), None);
    }

    #[test]
    fn test_parse_inline_with_tabs() {
        let input = b"GET\tmykey\r\n";
        let (frame, consumed) = parse(input).expect("parse inline tabs");
        assert_eq!(consumed, input.len());

        let mut args = frame.args();
        assert_eq!(args.next(), Some(&b"GET"[..]));
        assert_eq!(args.next(), Some(&b"mykey"[..]));
        assert_eq!(args.next(), None);
    }

    #[test]
    fn test_parse_inline_with_quotes() {
        let input = b"SET \"my spaced key\" 'my spaced val'\r\n";
        let (frame, consumed) = parse(input).expect("parse inline quotes");
        assert_eq!(consumed, input.len());

        let mut args = frame.args();
        assert_eq!(args.next(), Some(&b"SET"[..]));
        assert_eq!(args.next(), Some(&b"my spaced key"[..]));
        assert_eq!(args.next(), Some(&b"my spaced val"[..]));
        assert_eq!(args.next(), None);
    }

    #[test]
    fn test_parse_inline_bare_newline() {
        let input = b"INFO\n";
        let (frame, consumed) = parse(input).expect("parse bare newline");
        assert_eq!(consumed, 5);

        let mut args = frame.args();
        assert_eq!(args.next(), Some(&b"INFO"[..]));
        assert_eq!(args.next(), None);
    }

    #[test]
    fn test_command_args_from_array() {
        let input = b"*3\r\n$3\r\nSET\r\n$3\r\nfoo\r\n$3\r\nbar\r\n";
        let (frame, consumed) = parse(input).expect("parse command array");
        assert_eq!(consumed, input.len());

        let mut args = frame.args();
        assert_eq!(args.next(), Some(&b"SET"[..]));
        assert_eq!(args.next(), Some(&b"foo"[..]));
        assert_eq!(args.next(), Some(&b"bar"[..]));
        assert_eq!(args.next(), None);
    }

    #[test]
    fn test_pipelined_commands() {
        let input = b"+OK\r\n:10\r\n$4\r\nPING\r\n";
        let (f1, c1) = parse(input).expect("parse f1");
        assert_eq!(f1, RespFrame::SimpleString(b"OK"));
        assert_eq!(c1, 5);

        let remaining = &input[c1..];
        let (f2, c2) = parse(remaining).expect("parse f2");
        assert_eq!(f2, RespFrame::Integer(10));
        assert_eq!(c2, 5);

        let remaining2 = &remaining[c2..];
        let (f3, c3) = parse(remaining2).expect("parse f3");
        assert_eq!(f3, RespFrame::BulkString(b"PING"));
        assert_eq!(c3, 10);
    }

    #[test]
    fn test_incomplete_inputs() {
        assert_eq!(parse(b""), Err(ParseError::Incomplete));
        assert_eq!(parse(b"+"), Err(ParseError::Incomplete));
        assert_eq!(parse(b"+OK"), Err(ParseError::Incomplete));
        assert_eq!(parse(b":123"), Err(ParseError::Incomplete));
        assert_eq!(parse(b"$5\r\nfoo"), Err(ParseError::Incomplete));
        assert_eq!(parse(b"*2\r\n$3\r\nfoo"), Err(ParseError::Incomplete));
        assert_eq!(parse(b"PING"), Err(ParseError::Incomplete));
    }

    #[test]
    fn test_pipeline_iter_batch() {
        let input = b"PING\r\n+OK\r\n:42\r\n";
        let mut iter = parse_pipeline(input);

        assert_eq!(iter.next(), Some(Ok(RespFrame::Inline(InlineIter::new(b"PING")))));
        assert_eq!(iter.next(), Some(Ok(RespFrame::SimpleString(b"OK"))));
        assert_eq!(iter.next(), Some(Ok(RespFrame::Integer(42))));
        assert_eq!(iter.next(), None);
        assert_eq!(iter.consumed(), input.len());
        assert_eq!(iter.remaining(), b"");
    }

    #[test]
    fn test_pipeline_iter_incomplete() {
        let input = b"PING\r\n+OK\r\n:incomplete";
        let mut iter = parse_pipeline(input);

        assert_eq!(iter.next(), Some(Ok(RespFrame::Inline(InlineIter::new(b"PING")))));
        assert_eq!(iter.next(), Some(Ok(RespFrame::SimpleString(b"OK"))));
        assert_eq!(iter.next(), Some(Err(ParseError::Incomplete)));
        assert_eq!(iter.next(), None);
        assert_eq!(iter.consumed(), 11); // 6 (PING\r\n) + 5 (+OK\r\n)
    }

    #[test]
    fn test_parse_command_pubsub() {
        // SUBSCRIBE
        let (f1, _) = parse(b"*3\r\n$9\r\nSUBSCRIBE\r\n$4\r\nnews\r\n$6\r\nsports\r\n").unwrap();
        assert_eq!(
            parse_command(f1),
            Command::Subscribe(vec![b"news", b"sports"])
        );

        // SUBSCRIBE inline
        let (f2, _) = parse(b"SUBSCRIBE tech crypto\r\n").unwrap();
        assert_eq!(
            parse_command(f2),
            Command::Subscribe(vec![b"tech", b"crypto"])
        );

        // SUBSCRIBE wrong args
        let (f3, _) = parse(b"SUBSCRIBE\r\n").unwrap();
        assert_eq!(parse_command(f3), Command::WrongArgs("subscribe"));

        // UNSUBSCRIBE
        let (f4, _) = parse(b"*2\r\n$11\r\nUNSUBSCRIBE\r\n$4\r\nnews\r\n").unwrap();
        assert_eq!(parse_command(f4), Command::Unsubscribe(vec![b"news"]));

        // UNSUBSCRIBE no args (unsubscribe all)
        let (f5, _) = parse(b"UNSUBSCRIBE\r\n").unwrap();
        assert_eq!(parse_command(f5), Command::Unsubscribe(vec![]));

        // PUBLISH
        let (f6, _) = parse(b"*3\r\n$7\r\nPUBLISH\r\n$4\r\nnews\r\n$12\r\nbreaking: ok\r\n").unwrap();
        assert_eq!(
            parse_command(f6),
            Command::Publish {
                channel: b"news",
                message: b"breaking: ok",
            }
        );

        // PUBLISH inline
        let (f7, _) = parse(b"PUBLISH alerts fire!\r\n").unwrap();
        assert_eq!(
            parse_command(f7),
            Command::Publish {
                channel: b"alerts",
                message: b"fire!",
            }
        );

        // PUBLISH wrong args
        let (f8, _) = parse(b"PUBLISH news\r\n").unwrap();
        assert_eq!(parse_command(f8), Command::WrongArgs("publish"));
    }

    #[test]
    fn test_parse_command_crud_and_advanced() {
        let (f1, _) = parse(b"*3\r\n$3\r\nSET\r\n$3\r\nkey\r\n$3\r\nval\r\n").unwrap();
        assert_eq!(
            parse_command(f1),
            Command::Set {
                key: b"key",
                val: b"val",
                ttl_ms: None
            }
        );

        let (f2, _) = parse(b"SET key val EX 10\r\n").unwrap();
        assert_eq!(
            parse_command(f2),
            Command::Set {
                key: b"key",
                val: b"val",
                ttl_ms: Some(10000)
            }
        );

        let (f3, _) = parse(b"GET mykey\r\n").unwrap();
        assert_eq!(parse_command(f3), Command::Get(b"mykey"));

        let (f4, _) = parse(b"DEL mykey\r\n").unwrap();
        assert_eq!(parse_command(f4), Command::Del(b"mykey"));

        let (f5, _) = parse(b"HSET user name alice\r\n").unwrap();
        assert_eq!(
            parse_command(f5),
            Command::HSet {
                key: b"user",
                field: b"name",
                val: b"alice"
            }
        );

        let (f6, _) = parse(b"LPUSH queue task1\r\n").unwrap();
        assert_eq!(
            parse_command(f6),
            Command::LPush {
                key: b"queue",
                val: b"task1"
            }
        );

        let (f7, _) = parse(b"ZADD leaderboard 99.5 bob\r\n").unwrap();
        assert_eq!(
            parse_command(f7),
            Command::ZAdd {
                key: b"leaderboard",
                score: 99.5,
                member: b"bob"
            }
        );

        // Replication commands: PSYNC, SYNC, REPLCONF, ROLE, INFO
        let (f8, _) = parse(b"PSYNC ? -1\r\n").unwrap();
        assert_eq!(
            parse_command(f8),
            Command::PSync {
                replid: b"?",
                offset: -1,
            }
        );

        let (f9, _) = parse(b"SYNC\r\n").unwrap();
        assert_eq!(parse_command(f9), Command::Sync);

        let (f10, _) = parse(b"REPLCONF listening-port 6380\r\n").unwrap();
        assert_eq!(
            parse_command(f10),
            Command::ReplConf(vec![b"listening-port", b"6380"])
        );

        let (f11, _) = parse(b"ROLE\r\n").unwrap();
        assert_eq!(parse_command(f11), Command::Role);

        let (f12, _) = parse(b"INFO replication\r\n").unwrap();
        assert_eq!(parse_command(f12), Command::Info(Some(b"replication")));
    }
}
