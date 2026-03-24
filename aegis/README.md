# 🛡️ AEGIS: Ultra-High-Performance L4 TCP Proxy

AEGIS is a bare-metal TCP (L4) proxy written in pure Rust. Engineered to act as a resilient shield for high-throughput backend databases (specifically, a custom LSM-Tree engine named Chronos), it maximizes mechanical sympathy by minimizing context switches via asynchronous syscall batching and eliminating cross-core lock contention.



## 🧠 Architecture: The Silicon Sympathy Approach
AEGIS discards traditional global thread-pools and standard `epoll` event loops. It operates entirely on a **Thread-per-Core (Shared-Nothing)** paradigm to achieve absolute zero lock-contention and linear scalability.

* **Core Pinning (`core_affinity`):** Spawns exactly N OS threads (matching physical/logical cores) and magnetically pins them to the CPU silicon. CPU Cache L1/L2 is strictly preserved.
* **Hardware-Level Load Balancing (`SO_REUSEPORT`):** Raw sockets are forged in C (`socket2`) before entering Rust. We rely on the NIC and the Linux Kernel's hash algorithms to distribute incoming SYN packets directly to the isolated cores.
* **Syscall Batching (`io_uring`):** Uses `tokio-uring` for high-performance I/O, replacing traditional `epoll` loops with shared-memory Submission/Completion Queues to drastically reduce syscall overhead.
* **Zero-Allocation Hot Path:** Pre-allocated Thread-Local Memory Pools (Slab Allocators via `VecDeque`). Memory is recycled for active connections, ensuring deterministic behavior and zero `malloc` calls during the hot path of traffic routing.
* **Cache Line Padding:** Telemetry atomics are wrapped in `#[repr(align(64))]` to perfectly align with CPU cache lines, completely eradicating false sharing across cores.

## 📉 Performance Evolution: The TCP Handshake Bottleneck
During initial load testing, the proxy hit a wall at ~4,400 RPS. Profiling revealed that the `epoll` starvation wasn't the issue; the bottleneck was the repeated TCP handshakes (`SYN/ACK` cycles) to the backend database under high connection churn. 

By implementing a **thread-local persistent connection pool ("hot pipes")** in Phase 5, AEGIS bypassed backend connection teardowns, effectively doubling the throughput to 8,400+ RPS and cutting P99 latency by 55%.

## 🚀 The 5 Phases of AEGIS
- [x] **Phase 1: Silicon Topology.** Core detection, thread pinning, raw socket forging, and isolated `io_uring` runtimes.
- [x] **Phase 2: The Thermal Loop.** Zero-allocation hot path. Pre-allocated Thread-Local Memory Pools.
- [x] **Phase 3: Particle Accelerator.** L4 Routing and asynchronous data passing through Kernel shared rings.
- [x] **Phase 4: Stark HUD.** Lock-free atomic telemetry and Graceful Shutdown coordinated via a Broadcast Control Plane.
- [x] **Phase 5: Symbiosis.** Persistent connection pooling (Thread-Local Hot Pipes) with the Chronos LSM-Tree backend.

## 📊 Benchmarks
Tested on local consumer hardware (AMD Ryzen) routing traffic to a local LSM-Tree Database (Chronos). 

**Attack Vector (500k total requests across 200 concurrent connections):** `ab -k -n 500000 -c 200 http://127.0.0.1:8081/`

| Metric | Result | Note |
| :--- | :--- | :--- |
| **Complete Requests** | `500,000` | 100% Success Rate |
| **Failed Requests** | `0` | Zero dropped connections under load |
| **Requests per Second** | `~8,400+ [#/sec]` | Full Proxy + DB Round-Trips |
| **P99 Latency** | `< 1 ms` | 99% of requests routed in less than 1 millisecond |
| **Max Latency** | `4 ms` | Absolute worst-case scenario |

## 🛰️ Observability (Grafana & Prometheus)
AEGIS features a lock-free, zero-cost HTTP metrics satellite running on the Control Plane (`port 8082`). You can visualize RPS and Bandwidth in real-time without locking the data plane.

1. Boot the proxy and the backend:
```bash
cargo run --release
```
2. Launch the telemetry stack:

```bash
docker compose up -d
```

3. Open `http://localhost:3000` to view the live dashboard during load testing.
