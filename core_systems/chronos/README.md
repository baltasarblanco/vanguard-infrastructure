# 🦀 CHRONOS: High-Performance LSM Key-Value Store

![Rust](https://img.shields.io/badge/rust-stable-orange?style=flat-square)
![Architecture](https://img.shields.io/badge/architecture-LSM%20Tree-blue?style=flat-square)
![Concurrency](https://img.shields.io/badge/concurrency-RwLock%20%2F%20Multithreaded-green?style=flat-square)
![License](https://img.shields.io/badge/license-MIT-lightgrey?style=flat-square)

> *"Entropy is inevitable. Data loss is not."*

**Chronos** is a persistent, concurrent, and high-performance Key-Value store built from scratch in **Rust**. It implements a **Log-Structured Merge-Tree (LSM)** architecture similar to RocksDB or LevelDB, coupled with a custom **TCP Protocol** for remote access.

Designed as an educational deep-dive into systems engineering, Chronos bridges the gap between simple in-memory hashmaps and production-grade databases like Redis.

---

## ⚡ Key Features

### 🧠 **Symbiotic Architecture**
- **Hybrid Storage Engine:** Uses an in-memory `MemTable` (HashMap) for nanosecond-latency reads and disk-based `SSTables` for long-term storage.
- **Write-Ahead Log (WAL):** Guarantees **Durability (ACID)**. Every write is appended to a log file before acknowledgement. If the server crashes, Chronos replays the WAL upon restart to restore the state (0% Data Loss).
- **Tombstone Deletion:** High-efficiency `DEL` command implementation that uses memory tombstones to mark records as deleted without triggering expensive disk re-writes.

### 🚀 **High-Performance Concurrency**
- **Multithreaded Server:** Handles concurrent TCP connections using thread spawning and safe memory sharing.
- **Lock-Free Reads:** Implements `Arc<RwLock<T>>` to allow **multiple simultaneous readers** without blocking. Writers only block when absolutely necessary.
- **Graceful Shutdown:** Intercepts `SIGINT` (Ctrl+C) signals to safely block new connections, flush memory buffers to disk, and close TCP sockets without data corruption.

### 🛡️ **Self-Healing & Maintenance**
- **Crash Recovery:** Automatic "Rehydration" mechanism restores database state from disk on boot.
- **Live Compaction:** In-memory garbage collection to prune tombstones and duplicated logs, optimizing the read path.

---

## 🛠️ Architecture Overview

The system is composed of three distinct layers, completely decoupled:

1.  **The Interface (Network Layer - `server.rs`):** Raw TCP Sockets and Multithreading.
2.  **The Parser (Translation Layer - `parser.rs`):** Zero-copy parsing transforming raw bytes into strict Command Enums.
3.  **The Core (Storage Layer - `engine.rs`):** Volatile RAM storage and Append-only persistence file (`chronos.db`).

---

## 🚀 Quick Start

### 1. Clone the Repository
```bash
git clone [https://github.com/baltasarblanco/chronos_lsm.git](https://github.com/baltasarblanco/chronos_lsm.git)
cd chronos_lsm
```

## 2. Run the Server
```bash
cargo run
```
Expected Output:
```bash
🚀 CHRONOS SERVER LISTO Y ESCUCHANDO EN TCP 127.0.0.1:8080
```

## 3. Connect via the Interactive CLI
Chronos includes its own built-in terminal client (similar to redis-cli). Open a second terminal and run:
```bash
cargo run --bin client
```

## 4. Issue Commands
```text
chronos> SET user:101 {"name": "Venom", "role": "Symbiote"}
OK
chronos> GET user:101
{"name": "Venom", "role": "Symbiote"}
chronos> DEL user:101
OK_DELETED
chronos> COMPACT
OK_COMPACTED
```

## 🧪 Benchmarks & Performance (Local Dev Build - Release Mode)

Tests performed on local hardware via a single sequential TCP connection. Measured using 10,000 consecutive operations.

| Operation | Mechanism | Throughput (Sequential) | Latency | Outcome |
| :--- | :--- | :--- | :--- | :--- |
| Write (`SET`) | `RwLock` (Write) + Disk Append | **~10,500 ops/sec** | ~0.09 ms/op | Atomic Safety |
| Read (`GET`) | `RwLock` (Read) | **~15,000 ops/sec** | ~0.06 ms/op | Non-Blocking |
| Delete (`DEL`)| Tombstone Injection | O(1) in WAL | - | Zero Disk Rewrite |

*(Note: These metrics reflect raw sequential round-trips. Concurrent throughput via thread-pooling is significantly higher).*

## 👨‍💻 Author

Baltasar Blanco - Systems Engineer / Rustacean
Building infrastructure from the atom up.
