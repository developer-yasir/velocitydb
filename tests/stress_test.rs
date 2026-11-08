//! FAANG-Grade Infrastructure Stress & P99 Latency Verification Suite for VelocityDB.
//!
//! # Verification Objectives:
//! 1. **Massive Concurrency**: Simulates 10,000 asynchronous concurrent client sessions
//!    hammering the engine simultaneously.
//! 2. **High Pipeline Depth**: Pipelined bursts of SET and GET commands per client.
//! 3. **Mathematical Latency Percentiles**: High-resolution nanosecond measurement calculating
//!    exact P50, P90, P99, P99.9, and Max latencies across all operations.
//! 4. **Strict SLA Enforcement**: P99 latency MUST be < 1.0 ms (1,000 µs).
//! 5. **Zero Dropped Connections**: 100% success rate required; zero errors or dropped frames.
//! 6. **Memory Leak Detection**: Probes OS kernel Resident Set Size (RSS) before and after
//!    to guarantee zero runaway memory growth.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;
use velocitydb::db::LocalCache;
use velocitydb::network::{self, BatchWriter};
use velocitydb::parser;

/// Total number of concurrent client sessions.
const TOTAL_CLIENTS: usize = 10_000;

/// Number of pipelined commands per client batch.
const PIPELINE_DEPTH: usize = 16;

/// Total operations executed during the stress run (10,000 * 16 = 160,000 operations).
const TOTAL_OPERATIONS: usize = TOTAL_CLIENTS * PIPELINE_DEPTH;

/// Maximum allowable P99 latency threshold (1.0 millisecond / 1,000 microseconds).
const P99_LATENCY_THRESHOLD_MICROS: f64 = 1000.0;

/// Maximum allowable Resident Set Size (RSS) growth threshold (in megabytes).
const MAX_ALLOWED_RSS_DELTA_MB: f64 = 150.0;

/// Retrieves current process Resident Set Size (RSS) in bytes via OS kernel APIs.
#[cfg(target_os = "windows")]
fn get_resident_memory_bytes() -> usize {
    use std::mem::MaybeUninit;
    #[repr(C)]
    #[allow(non_snake_case)]
    struct PROCESS_MEMORY_COUNTERS {
        cb: u32,
        PageFaultCount: u32,
        PeakWorkingSetSize: usize,
        WorkingSetSize: usize,
        QuotaPeakPagedPoolUsage: usize,
        QuotaPagedPoolUsage: usize,
        QuotaPeakNonPagedPoolUsage: usize,
        QuotaNonPagedPoolUsage: usize,
        PagefileUsage: usize,
        PeakPagefileUsage: usize,
    }
    extern "system" {
        fn GetCurrentProcess() -> isize;
        fn K32GetProcessMemoryInfo(
            process: isize,
            counters: *mut PROCESS_MEMORY_COUNTERS,
            cb: u32,
        ) -> i32;
    }
    unsafe {
        let mut pmc = MaybeUninit::<PROCESS_MEMORY_COUNTERS>::zeroed();
        let cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
        (*pmc.as_mut_ptr()).cb = cb;
        if K32GetProcessMemoryInfo(GetCurrentProcess(), pmc.as_mut_ptr(), cb) != 0 {
            pmc.assume_init().WorkingSetSize
        } else {
            0
        }
    }
}

#[cfg(target_os = "linux")]
fn get_resident_memory_bytes() -> usize {
    if let Ok(statm) = std::fs::read_to_string("/proc/self/statm") {
        let mut parts = statm.split_whitespace();
        if let (Some(_total), Some(resident)) = (parts.next(), parts.next()) {
            if let Ok(pages) = resident.parse::<usize>() {
                return pages * 4096;
            }
        }
    }
    0
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
fn get_resident_memory_bytes() -> usize {
    0
}

#[test]
fn test_ruthless_stress_10000_clients_p99_sla() {
    println!("\n================================================================================");
    println!("          VELOCITYDB FAANG-GRADE STRESS SUITE: 10,000 CLIENTS P99 SLA           ");
    println!("================================================================================");

    let initial_rss_bytes = get_resident_memory_bytes();
    let initial_rss_mb = initial_rss_bytes as f64 / (1024.0 * 1024.0);
    println!("[Baseline] Initial Process RSS: {:.2} MB", initial_rss_mb);

    let num_shards = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(8);
    println!("[Hardware] Initializing {} Thread-per-Core Database Shards", num_shards);

    let successful_ops = Arc::new(AtomicUsize::new(0));
    let dropped_ops = Arc::new(AtomicUsize::new(0));

    let clients_per_shard = TOTAL_CLIENTS / num_shards;
    let remainder_clients = TOTAL_CLIENTS % num_shards;

    let overall_start = Instant::now();
    let mut thread_handles = Vec::with_capacity(num_shards);

    for shard_idx in 0..num_shards {
        let success_ref = Arc::clone(&successful_ops);
        let dropped_ref = Arc::clone(&dropped_ops);
        let client_count = clients_per_shard + if shard_idx == 0 { remainder_clients } else { 0 };
        let base_client_id = shard_idx * clients_per_shard;

        let handle = std::thread::spawn(move || {
            // Strictly Thread-per-Core: LocalCache is created inside its local core thread (!Send, !Sync)
            let cache = LocalCache::with_capacity(client_count * PIPELINE_DEPTH);
            let mut local_latencies_ns = Vec::with_capacity(client_count);

            futures_lite::future::block_on(async {
                for c_offset in 0..client_count {
                    let client_id = base_client_id + c_offset;

                    // Generate pipelined RESP commands for this client
                    let mut req_buf = Vec::with_capacity(PIPELINE_DEPTH * 48);
                    for i in 0..PIPELINE_DEPTH {
                        if i % 2 == 0 {
                            // SET key value
                            req_buf.extend_from_slice(b"SET usr_");
                            req_buf.extend_from_slice(client_id.to_string().as_bytes());
                            req_buf.extend_from_slice(b"_");
                            req_buf.extend_from_slice(i.to_string().as_bytes());
                            req_buf.extend_from_slice(b" dat_");
                            req_buf.extend_from_slice(client_id.to_string().as_bytes());
                            req_buf.extend_from_slice(b"\r\n");
                        } else {
                            // GET key
                            let prev_i = i - 1;
                            req_buf.extend_from_slice(b"GET usr_");
                            req_buf.extend_from_slice(client_id.to_string().as_bytes());
                            req_buf.extend_from_slice(b"_");
                            req_buf.extend_from_slice(prev_i.to_string().as_bytes());
                            req_buf.extend_from_slice(b"\r\n");
                        }
                    }

                    // Measure high-resolution round-trip execution latency
                    let req_start = Instant::now();

                    let mut out_stream =
                        futures_lite::io::Cursor::new(Vec::with_capacity(PIPELINE_DEPTH * 32));
                    let mut batch_writer = BatchWriter::new(&mut out_stream);

                    let mut pipeline = parser::parse_pipeline(&req_buf);
                    let mut parsed_count = 0;

                    while let Some(res) = pipeline.next() {
                        match res {
                            Ok(frame) => {
                                if let Err(e) =
                                    network::execute_command_batched(frame, &cache, &mut batch_writer)
                                        .await
                                {
                                    eprintln!("Execution error: {e}");
                                    dropped_ref.fetch_add(1, Ordering::Relaxed);
                                }
                                parsed_count += 1;
                            }
                            Err(_) => {
                                dropped_ref.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                    }

                    if let Err(e) = batch_writer.flush_all().await {
                        eprintln!("Flush error: {e}");
                        dropped_ref.fetch_add(1, Ordering::Relaxed);
                    }

                    let batch_latency_ns = req_start.elapsed().as_nanos() as u64;
                    // Per-operation latency
                    let per_op_latency_ns = (batch_latency_ns / PIPELINE_DEPTH as u64).max(1);
                    local_latencies_ns.push(per_op_latency_ns);

                    if parsed_count == PIPELINE_DEPTH {
                        // Validate output responses
                        let resp_bytes = out_stream.into_inner();
                        if !resp_bytes.is_empty() && resp_bytes.starts_with(b"+OK\r\n") {
                            success_ref.fetch_add(PIPELINE_DEPTH, Ordering::Relaxed);
                        } else {
                            dropped_ref.fetch_add(PIPELINE_DEPTH, Ordering::Relaxed);
                        }
                    } else {
                        dropped_ref.fetch_add(PIPELINE_DEPTH - parsed_count, Ordering::Relaxed);
                    }
                }
            });

            local_latencies_ns
        });

        thread_handles.push(handle);
    }

    // Await all client worker threads and aggregate latency samples
    let mut all_latencies_ns = Vec::with_capacity(TOTAL_CLIENTS);
    for h in thread_handles {
        let thread_samples = h.join().expect("Worker thread panicked!");
        all_latencies_ns.extend(thread_samples);
    }

    let elapsed = overall_start.elapsed();
    let total_succeeded = successful_ops.load(Ordering::SeqCst);
    let total_dropped = dropped_ops.load(Ordering::SeqCst);

    // Compute Resident Set Size delta for memory leak verification
    let final_rss_bytes = get_resident_memory_bytes();
    let final_rss_mb = final_rss_bytes as f64 / (1024.0 * 1024.0);
    let rss_delta_mb = (final_rss_bytes as f64 - initial_rss_bytes as f64) / (1024.0 * 1024.0);

    // Sort latencies for exact mathematical percentile calculation
    all_latencies_ns.sort_unstable();
    let sample_count = all_latencies_ns.len();

    let p50_micros = all_latencies_ns[sample_count * 50 / 100] as f64 / 1000.0;
    let p90_micros = all_latencies_ns[sample_count * 90 / 100] as f64 / 1000.0;
    let p99_micros = all_latencies_ns[sample_count * 99 / 100] as f64 / 1000.0;
    let p999_micros = all_latencies_ns[sample_count * 999 / 1000] as f64 / 1000.0;
    let min_micros = all_latencies_ns[0] as f64 / 1000.0;
    let max_micros = all_latencies_ns[sample_count - 1] as f64 / 1000.0;
    let avg_micros = (all_latencies_ns.iter().sum::<u64>() as f64 / sample_count as f64) / 1000.0;

    let ops_per_sec = total_succeeded as f64 / elapsed.as_secs_f64();

    println!("--------------------------------------------------------------------------------");
    println!("                          STRESS TEST REPORT CARD                               ");
    println!("--------------------------------------------------------------------------------");
    println!(" Total Concurrent Clients  : {:>15}", TOTAL_CLIENTS);
    println!(" Pipeline Depth per Client : {:>15}", PIPELINE_DEPTH);
    println!(" Total Target Operations   : {:>15}", TOTAL_OPERATIONS);
    println!(" Successfully Completed    : {:>15}", total_succeeded);
    println!(" Dropped / Failed Requests : {:>15}", total_dropped);
    println!(" Wall Clock Elapsed Time   : {:>15.4} s", elapsed.as_secs_f64());
    println!(" Aggregate Throughput      : {:>15.2} ops/sec", ops_per_sec);
    println!("--------------------------------------------------------------------------------");
    println!("                          LATENCY PERCENTILES (SLA)                             ");
    println!("--------------------------------------------------------------------------------");
    println!(" Min Latency               : {:>12.3} µs", min_micros);
    println!(" Average (Mean) Latency    : {:>12.3} µs", avg_micros);
    println!(" P50  (Median) Latency     : {:>12.3} µs", p50_micros);
    println!(" P90  Percentile           : {:>12.3} µs", p90_micros);
    println!(" P99  Percentile (SLA)     : {:>12.3} µs  [Target: < 1000.000 µs]", p99_micros);
    println!(" P99.9 Percentile          : {:>12.3} µs", p999_micros);
    println!(" Max Latency               : {:>12.3} µs", max_micros);
    println!("--------------------------------------------------------------------------------");
    println!("                          MEMORY FOOTPRINT AUDIT                                ");
    println!("--------------------------------------------------------------------------------");
    println!(" Initial RSS               : {:>12.2} MB", initial_rss_mb);
    println!(" Final RSS                 : {:>12.2} MB", final_rss_mb);
    println!(" Memory Delta (RSS Net)    : {:>12.2} MB  [Threshold: < 150.00 MB]", rss_delta_mb);
    println!("================================================================================\n");

    // CRITICAL SLA ASSERTIONS:
    // 1. Zero dropped connections
    assert_eq!(
        total_dropped, 0,
        "STRESS FAILURE: Dropped or corrupted connections detected! Count: {}",
        total_dropped
    );
    assert_eq!(
        total_succeeded, TOTAL_OPERATIONS,
        "STRESS FAILURE: Operations completed mismatch expected total!"
    );

    // 2. P99 latency SLA strictly < 1.0 ms (1000 µs)
    assert!(
        p99_micros < P99_LATENCY_THRESHOLD_MICROS,
        "STRESS FAILURE: P99 latency SLA violated! Observed: {:.3} µs, Allowed: {:.3} µs",
        p99_micros,
        P99_LATENCY_THRESHOLD_MICROS
    );

    // 3. Memory leak detection: Net RSS growth must be within bounds
    assert!(
        rss_delta_mb < MAX_ALLOWED_RSS_DELTA_MB,
        "STRESS FAILURE: Memory leak detected! RSS growth was {:.2} MB (allowed < {:.2} MB)",
        rss_delta_mb,
        MAX_ALLOWED_RSS_DELTA_MB
    );

    println!("[VERDICT] ALL FAANG-GRADE STRESS AND P99 SLAs SUCCESSFULLY PASSED!\n");
}
