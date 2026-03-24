# 🛡️ VANGUARD: High-Performance Infrastructure Suite

A bare-metal ecosystem built in **Rust** for ultra-low latency network processing, real-time threat detection, and deterministic storage.

---

## 🏗️ The Architecture

- **AEGIS**: A Layer 4 TCP Proxy utilizing `io_uring` and a Thread-per-Core model. It eliminates cross-core lock contention and minimizes syscall overhead.
- **CELER**: A Complex Event Processing (CEP) engine processing **~85 Million Packets/Sec**. It uses zero-copy IPC and SPSC lock-free ring buffers with cache-line padding (64-byte) to prevent false sharing.
- **RYŪ**: Real-time telemetry dashboard with WebSocket streaming for instant L7 threat visualization.
- **CHRONOS**: High-performance LSM-tree storage engine designed for high-write throughput.

```text
                                [ ETHERNET / RAW SOCKETS ]
                                            |
       _____________________________________V_____________________________________
      |                                                                           |
      |          AEGIS PROXY (L4) - io_uring & Thread-per-Core                    |
      |   [ Core 0 ]  [ Core 1 ]  [ Core 2 ]  [ Core 3 ] ... [ Core 31 ]          |
      |_______|___________|___________|___________|_____________|_________________|
              |           |           |           |             |
              |           |           |           |             |  [ SCM_RIGHTS ]
              |           |           |           |             |  [ Zero-Copy  ]
       _______V___________V___________V___________V_____________V_________________
      |                                                                           |
      |          CELER ENGINE (CEP) - 85 Mpps Centinel                            |
      |   [ Lock-Free Ring Buffer ] <--- [ Fast-Path IP Tracking (O(1)) ]         |
      |_________________|_______________________________________|_________________|
                        |                                       |
          ______________|______________           ______________|______________
         |                             |         |                             |
         |   CHRONOS DB (LSM-Tree)     |         |    RYŪ DASHBOARD (Web)      |
         |      (Persistence)          |         |     (Real-time Visuals)     |
         |_____________________________|         |_____________________________|        
```

## ⚡ Technical Deep Dive

- **Zero-Copy IPC Bridge**: Custom implementation using memfd_create and SCM_RIGHTS for file descriptor passing between isolated processes.
- **Mechanical Sympathy**: Hot paths designed to be zero-allocation, leveraging pre-allocated slab pools and cache-local data structures.
- **O(1) Mitigation**: Constant-time IP tracking and SYN-flood detection on the network hot path.

---

## 🚀 Performance & Benchmarks

Theoretical Peak (Lab Environment)

| Metric                | Value                      |
|-----------------------|----------------------------|
| IPC Throughput        | **85.37 Mpps** (wire-speed)|
| Mitigation Latency    | `<12 ms` for 1M event burst|
| Memory Profile        | **Deterministic**, zero-malloc during active attack mitigation |

Real-World Baseline (Local Machine Proof)
    Environment: Pop!_OS 22.04 | 100M Event Burst | No Core Pinning

- **Peak Throughput**: `27.98 Mpps`
- **Processing Speed**: ~35.7 nanoseconds per network event.
- **Packet Loss**: 0.00% (Zero-Copy Ring Buffer stability).

``(Insert your telemetry screenshot here to show the 27.98 Mpps in action)``

---

## 🔧 Getting Started

```bash
# Clone the repository
git clone [https://github.com/baltasarblanco/vanguard-infrastructure](https://github.com/baltasarblanco/vanguard-infrastructure)
cd vanguard-infrastructure

# Build the workspace in release mode
cargo build --release

# Run components (In separate terminals)
./target/release/chronos_lsm
./target/release/celer_mock
./target/release/aegis_proxy
```
    Note: Requires Linux kernel 6.x+ and io_uring support. See docs for detailed deployment guides.

## 📄 License

This project is licensed under the MIT License. See LICENSE for details.


