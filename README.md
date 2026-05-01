# 🛡️ Vanguard — Infrastructure Suite

Rust ecosystem for L4 network processing, complex event detection, and high-write storage.

---

## 🏗️ The Architecture

- **AEGIS**: A Layer 4 TCP Proxy utilizing `io_uring` and a Thread-per-Core model. It eliminates cross-core lock contention and minimizes syscall overhead.
- **CELER**: A Complex Event Processing (CEP) engine (lab peak 85 Mpps, sustained 28.97 Mpps). It uses zero-copy IPC...
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
      |          CELER ENGINE (CEP) - 28.97 Mpps sustained                            |
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

## Repositories

| Component | Repo | Numbers |
|-----------|------|---------|
| AEGIS (L4 TCP proxy) | [aegis-proxy](https://github.com/baltasarblanco/aegis-proxy) | 8.4k RPS, P99<1ms |
| CELER (CEP engine) | [celer_mock](https://github.com/baltasarblanco/celer_mock) | 28.97 Mpps sustained, ~34.5 ns/event |
| CHRONOS (KV store) | [chronos_lsm](https://github.com/baltasarblanco/chronos_lsm) | 15k reads/s, 10.5k writes/s |
| RYŪ (dashboard) | this repo | WebSocket frontend live, backend pending |

## Real-World Baseline (CELER)
- Throughput: **28.97 Mpps** sustained over 100M events
- Latency: **~34.5 ns** per packet
- IPC: memfd_create + SCM_RIGHTS (zero-copy)
- Hardware: Pop!_OS 22.04, Ryzen, core pinning (taskset -c 2,4)

## License
MIT