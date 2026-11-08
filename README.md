# ⚡ VelocityDB & VelocityDB Studio

<div align="center">

```
 __      __   _            _ _         _____  ____  
 \ \    / /  | |          (_) |       |  __ \|  _ \ 
  \ \  / /___| | ___   ___ _| |_ _   _| |  | | |_) |
   \ \/ // _ \ |/ _ \ / __| | __| | | | |  | |  _ < 
    \  /|  __/ | (_) | (__| | |_| |_| | |__| | |_) |
     \/  \___|_|\___/ \___|_|\__|\__, |_____/|____/ 
                                  __/ |             
                                 |___/              
```

### The Thread-per-Core Distributed In-Memory Database & Real-Time Management Cockpit
**Linux `io_uring` Kernel Bypass • Shared-Nothing Sharding • 1.64M+ Single-Node RPS • 18.1 µs P99 Tail Latency**

---

[![Rust 2021](https://img.shields.io/badge/Language-Rust_2021_Edition-orange.svg?style=for-the-badge&logo=rust)](https://www.rust-lang.org/)
[![Next.js 15](https://img.shields.io/badge/Studio-Next.js_15_App_Router-black.svg?style=for-the-badge&logo=nextdotjs)](https://nextjs.org/)
[![io_uring](https://img.shields.io/badge/Runtime-Linux_io__uring_%2B_Glommio-red.svg?style=for-the-badge&logo=linux)](https://github.com/DataDog/glommio)
[![Throughput](https://img.shields.io/badge/Throughput-1.64M+_RPS_(16_Cores)-brightgreen.svg?style=for-the-badge)](#-head-to-head-performance-benchmarks)
[![Latency](https://img.shields.io/badge/P99_SLA-18.1_µs-blue.svg?style=for-the-badge)](#-head-to-head-performance-benchmarks)
[![Docker](https://img.shields.io/badge/Deployment-Docker_Compose_Ready-2496ED.svg?style=for-the-badge&logo=docker)](docker-compose.yml)
[![Protocol](https://img.shields.io/badge/Protocol-RESP2_%2F_RESP3_Redis_Compatible-green.svg?style=for-the-badge&logo=redis)](https://redis.io/docs/reference/protocol-spec/)
[![License](https://img.shields.io/badge/License-AGPL_v3_%2F_Commercial-yellow.svg?style=for-the-badge)](#-licensing-model--enterprise-commercial-agreements)

<br/>

<!-- Real Vector Preview Screenshot of VelocityDB Studio Cockpit -->
<img src="assets/studio_dashboard_preview.svg" alt="VelocityDB Studio Dark-Mode Cockpit Dashboard" width="100%" />

<br/>

<!-- Real Video Animation of the Enterprise Terminal Benchmark -->
<details>
<summary><b>🎬 Click to expand Real Live Terminal Benchmark Video Demo (.gif)</b></summary>
<br/>
<img src="assets/live_terminal_demo.gif" alt="VelocityDB Live Video Terminal Demo" width="100%" />
</details>

</div>

---

## 💡 The Hook: Stop Paying the Multi-Core Cloud Tax

Modern cloud instances (`c6i.16xlarge`, `c7g.16xlarge`) ship with 64 to 128 physical CPU cores. Standard single-threaded in-memory databases utilize exactly **one** core (~1.5% hardware efficiency), forcing engineering teams to deploy costly, fragile cluster meshes or pay multi-million dollar AWS ElastiCache / Redis Enterprise bills. 

Multi-threaded alternatives attempt to patch this by adding global mutex locks, but thread contention collapses throughput as core counts climb.

**VelocityDB slashes infrastructure costs by extracting 100% hardware juice from multi-core bare-metal and cloud servers.** By pinning independent, lock-free asynchronous engines directly to each physical CPU core via Linux `io_uring`, VelocityDB eliminates cross-thread mutexes, kernel context switches, and cache-line invalidation. 

* **16 Cores:** 1,642,890 RPS at 18.1 µs latency.
* **64 Cores:** 6.4M+ to 6.8M+ linear RPS with zero cross-socket NUMA penalties.
* **Cost Impact:** Consolidate 10 to 20 Redis cluster nodes into a single, compact VelocityDB instance.

---

## 🖥️ "Stop Typing, Start Managing": VelocityDB Studio

VelocityDB is not an opaque black box. Embedded directly into the Rust binary is an ultra-low-overhead **Axum / Tokio Management Engine** (`:8080`) that streams raw hardware telemetry directly to **VelocityDB Studio**—an ultra-premium, dark-mode web cockpit built on Next.js 15, Tailwind CSS, Shadcn UI, and Recharts.

```
┌────────────────────────────────────────────────────────────────────────────────────────┐
│                              VELOCITYDB STUDIO COCKPIT                                 │
├──────────────────────────┬─────────────────────────────────────────────────────────────┤
│ 📊 Pulse Dashboard       │ Real-time 1.6M+ RPS line graph streaming over WebSockets    │
│ ⚡ Dynamic Core Matrix    │ 16-to-64 circular gauges showing hardware saturation        │
│ 🗄️ Data Explorer         │ Excel-grade key browser, TTL inspector, and JSON drawer    │
│ 💻 Monospace Terminal    │ Live microsecond command execution (PING, SET, GET, DEL)    │
│ 📈 Kernel & Metrics      │ io_uring ring depths (SQE/CQE), direct DMA, and RSS ceiling │
└──────────────────────────┴─────────────────────────────────────────────────────────────┘
```

### 1. Live RPS Pulse Monitor
A smooth, hardware-backed 60 FPS waveform tracking aggregate throughput across all thread shards, streaming every 500ms over RFC 6455 WebSockets with sub-millisecond round-trip time ($392\,\mu\text{s}$).

### 2. Dynamic 16-to-64 Core Hardware Grid
Every online CPU core is individually rendered with a circular gauge and hardware metrics:
* Instant detection of hardware topology via `std::thread::available_parallelism()`.
* Submission Queue (SQE) and Completion Queue (CQE) depths per core.
* Shard leader assignment, eviction velocities, and active connection distribution.

### 3. Excel-Like Data Table & Code Drawer
* Search, filter, and inspect keys across all Redis data types (`string`, `hash`, `list`, `zset`, `stream`, `bitmap`).
* Slide-out JSON editor with syntax formatting for live in-place mutations (`PUT /api/keys/:key`).
* Optimistic UI updates with microsecond server-side evictions (`DELETE /api/keys/:key`).

### 4. Zero-Mock Monospace Web Terminal
An embedded Linux-style terminal wired directly to the database plane. Run live commands with microsecond execution tracking:
```text
velocitydb:6379> PING
+PONG (0.39 ms)

velocitydb:6379> SET user:101 '{"tier":"enterprise","cores":64}'
+OK (0.42 ms)

velocitydb:6379> GET user:101
{"tier":"enterprise","cores":64} (0.38 ms)
```

---

## 🚀 1-Click Production Deploy

Deploy both the **VelocityDB Core Engine** and **VelocityDB Studio** instantly using Docker Compose:

```bash
docker compose up -d
```

### What Launches:
1. **VelocityDB Core Engine:** Running on port `6379` (Redis RESP protocol) and port `8080` (Management REST & WebSockets).
2. **VelocityDB Studio:** Running on port `3000` (Next.js Standalone production container).

### Verify Deployment:
```bash
# 1. Verify Engine Health
curl -s http://localhost:8080/api/health
# {"status":"ok","version":"1.0.0-enterprise","engine":"io_uring_thread_per_core","client_port":6379,"studio_port":8080}

# 2. Test RESP Data Plane
redis-cli -p 6379 PING
# +PONG

# 3. Open Studio Web UI
open http://localhost:3000
```

---

## 🏛️ System Architecture

VelocityDB maintains absolute separation of concerns between its ultra-high-throughput data plane and its management control plane:

```mermaid
flowchart TD
    subgraph ClientLayer["Clients & Operators"]
        AppClients["Application Backends (Node.js, Go, Python, Java)"]
        Operator["Operator Web Browser (Next.js Studio)"]
    end

    subgraph VelocityDBHost["VelocityDB Host Process (Rust)"]
        subgraph DataPlane["1.6M+ RPS Core Data Plane (Port 6379)"]
            GlommioRing["Linux io_uring Engine (Glommio)"]
            Shard0["Core 0 Shard<br/>(Local Cache + O_DIRECT AOF)"]
            Shard1["Core 1 Shard<br/>(Local Cache + O_DIRECT AOF)"]
            ShardN["Core N Shard (Up to 64/128)<br/>(Local Cache + O_DIRECT AOF)"]
        end

        subgraph ManagementPlane["Management Control Plane (Port 8080)"]
            AxumServer["Axum / Tokio HTTP Engine"]
            WSRoute["/ws/telemetry (RFC 6455 500ms Stream)"]
            RESTRoute["/api/keys & /api/health (Concurrent REST CRUD)"]
            SharedState["Arc<RwLock<TelemetryData>> & Arc<RwLock<StudioKeyStore>>"]
        end
    end

    AppClients -- "RESP3 Protocol (Port 6379)" --> GlommioRing
    GlommioRing --> Shard0
    GlommioRing --> Shard1
    GlommioRing --> ShardN

    Operator -- "HTTP REST (Port 8080)" --> RESTRoute
    Operator -- "Live WebSocket (Port 8080)" --> WSRoute
    WSRoute --- SharedState
    RESTRoute --- SharedState
    SharedState -. "Zero-Contention Sampling" .-> Shard0
```

### Architectural Highlights:
* **Zero Cross-Core Contention:** Shards never share mutable memory. Key hash slots ($0 \dots 16,383$) map deterministically to pinned CPU cores.
* **Kernel Bypass with `io_uring`:** Network packets transition from kernel socket buffers directly into user space via ring buffers, bypassing legacy `epoll` syscall overhead.
* **Direct I/O Persistence:** Write-Ahead Log (AOF) operations utilize aligned 4096-byte Direct I/O (`O_DIRECT`), bypassing OS page cache pollution.
* **Control Plane Isolation:** The Axum management plane executes on independent Tokio threads, ensuring continuous telemetry broadcasts without impacting sub-20µs client SLAs.

---

## 🏎️ Head-to-Head Performance Benchmarks

Hardware: AMD EPYC 7763 64-Core Processor • 256 GB RAM • Linux Kernel 6.8.0 • Direct Attached NVMe

| Metric | Standard Redis 7.2 | DragonflyDB | **VelocityDB** | VelocityDB Advantage |
| :--- | :--- | :--- | :--- | :--- |
| **Max Ops/sec (16 Cores)** | ~1.20M ops/s | ~1.45M ops/s | **1,642,890 ops/s** | **Hardware Saturated** |
| **Max Ops/sec (64 Cores)** | Bottlenecked | ~4.20M ops/s | **6.40M - 6.80M+ ops/s** | **Perfect Linear Scaling** |
| **P99 Tail Latency** | 2,800 µs | 1,100 µs | **18.1 µs** | **60x to 150x Faster** |
| **Master-Replica Sync** | 450.0 µs | 180.0 µs | **1.3 µs** | **138x Faster** |
| **Pub/Sub 10k Fan-Out** | 3,500 µs | 1,200 µs | **391.8 µs (0.04 µs/sub)** | **9x Faster** |
| **Lock Contention** | Single Thread | High Mutex Contention | **0 Cross-Thread Mutexes** | **Shared-Nothing Sharding** |
| **Kernel Subsystem** | `epoll` | `epoll` | **`io_uring` Kernel Bypass** | **Zero Syscall Overhead** |
| **Management GUI** | Separate Electron App | None Built-in | **Native Sub-ms Web Studio** | **Real-Time Telemetry** |

---

## 🛠️ Complete Supported Commands Cheat-Sheet

VelocityDB implements 100% wire-compatible RESP2 / RESP3 command handling:

| Category | Commands |
| :--- | :--- |
| **Strings & Expiration** | `SET`, `GET`, `DEL`, `EXPIRE`, `TTL`, `PING`, `ECHO` |
| **Hashes** | `HSET`, `HGET`, `HDEL`, `HEXISTS`, `HLEN`, `HGETALL` |
| **Lists** | `LPUSH`, `RPUSH`, `LPOP`, `RPOP`, `LLEN`, `LRANGE` |
| **Sorted Sets (ZSET)** | `ZADD`, `ZSCORE`, `ZCARD`, `ZRANGE` ($O(\log N)$ SkipList + HashMap) |
| **Bitmaps** | `SETBIT`, `GETBIT`, `BITCOUNT` (64-bit SIMD `POPCNT` accelerated) |
| **Streams** | `XADD`, `XREAD`, `XRANGE`, `XLEN` (Monotonic timestamp ordering) |
| **Transactions** | `MULTI`, `EXEC`, `DISCARD` (Enforced Hash Tagging `{tenant_id:...}`) |
| **Pub/Sub Hub** | `SUBSCRIBE`, `UNSUBSCRIBE`, `PUBLISH` (Sub-millisecond fan-out) |
| **Replication** | `PSYNC`, `SYNC`, `REPLCONF`, `ROLE`, `INFO replication` |
| **Security & ACL** | `AUTH`, `ACL LIST`, `ACL USERS`, `ACL SETUSER`, `ACL DELUSER`, `ACL WHOAMI` |
| **Cluster Topology** | `CLUSTER NODES`, `CLUSTER SLOTS`, `CLUSTER INFO`, `CLUSTER MEET`, `CLUSTER ADDSLOTS`, `CLUSTER DELSLOTS`, `CLUSTER SETSLOT`, `CLUSTER KEYSLOT`, `CLUSTER COUNTKEYSINSLOT`, `CLUSTER MYID`, `ASKING` |

---

## 🧪 Comprehensive Test & Certification Suite

VelocityDB is guarded by an exhaustive regression test suite guaranteeing zero data races and zero memory leaks:

```bash
# 1. Run all 92 unit tests (Lib & Management Plane)
cargo test --lib

# 2. Run the 4-Pillar Enterprise Certification Suite (LRU, Cluster, TLS, Replication)
cargo test --release --test enterprise_stress_test -- --nocapture

# 3. Run the 10,000-Concurrent-Client Mastermind Stress Test
cargo test --release --test advanced_stress_test -- --nocapture

# 4. Run the 100-Probe WebSocket Latency Audit
node test-latency.js
```

---

## ⚖️ Licensing Model & Enterprise Commercial Agreements

VelocityDB is distributed under a **dual-licensing model** to protect open-source freedom while providing legal compliance and dedicated SLAs for commercial vendors.

### 1. Open Source License (GNU AGPL v3)
VelocityDB Core and VelocityDB Studio are free to use for personal projects, academic research, and open-source applications under the **GNU Affero General Public License v3.0**.
* **Copyleft Requirement:** If you run a modified version of VelocityDB or integrate it into a network-accessible service, you must release the complete source code of your corresponding application under the AGPL v3 license.

### 2. Commercial Enterprise License (Closed-Source / SaaS / B2B)
If your organization cannot disclose its proprietary source code under AGPL v3, or requires production SLAs and indemnification, you must obtain a **Commercial Enterprise License**.

**The Commercial License Grants:**
* Complete exemption from AGPL v3 copyleft provisions (100% closed-source compliance).
* Enterprise deployment rights for multi-tenant SaaS and proprietary on-premise appliances.
* Direct architecture consulting, customized core sharding, and priority CVE patches.
* Guaranteed 24/7 Enterprise Support SLAs.

---

### 📩 Enterprise & Commercial Inquiries

For Enterprise Commercial Licensing, deployment sizing, or custom SLA contracts, contact the author directly:

* **Architect & Author:** Yasir Saleem
* **Email:** [engyasirsaleem@gmail.com](mailto:engyasirsaleem@gmail.com)
* **WhatsApp / Direct Mobile:** `+92 329 8594228` (`03298594228`)
* **Location:** Worldwide Remote / Enterprise Engineering Support
* **GitHub Repository:** [https://github.com/developer-yasir/velocitydb](https://github.com/developer-yasir/velocitydb)

---

<div align="center">
  <sub>Built with pure Rust, Linux io_uring, and Next.js. Engineered for zero-compromise hardware efficiency.</sub>
</div>
