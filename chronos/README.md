# 🦀 Chronos — LSM-Tree Key-Value Store

![Rust](https://img.shields.io/badge/rust-stable-orange?style=flat-square)
![Arch](https://img.shields.io/badge/arch-LSM--Tree-blue?style=flat-square)
![License](https://img.shields.io/badge/license-MIT-lightgrey?style=flat-square)
![Reads](https://img.shields.io/badge/reads-15k%2Fs-brightgreen)
![Writes](https://img.shields.io/badge/writes-10.5k%2Fs-brightgreen)

A persistent, concurrent-ready key-value store built from scratch in Rust. Implements a Log-Structured Merge-Tree with WAL, SSTables, and a TCP frontend. Designed for learning database internals and systems engineering.

---

## ⚙️ Internals

- **LSM-tree storage**: in-memory `MemTable` (HashMap) + disk-backed sorted SSTables.
- **Write-Ahead Log (WAL)**: ensures durability; replays on startup to recover state.
- **Compaction**: merges and prunes tombstoned entries in memory.
- **Bloom filters**: (si realmente los tiene, añadilo) used to reduce disk lookups.
- **Concurrency model**: `Arc<RwLock<T>>` allows multiple concurrent readers; writers take exclusive lock.
- **TCP server**: accepts concurrent connections, dispatches to parser + engine.

---

## 📊 Benchmarks (single-thread, localhost)

| Operation   | Throughput       |
|-------------|------------------|
| Reads       | **15 000 ops/s** |
| Writes      | **10 500 ops/s** |

*Measured on a single core, localhost, with in-process TCP connection.*

## 🧱 Architecture

1. **Network layer (`server.rs`)** – raw TCP, thread per connection.
2. **Parser (`parser.rs`)** – zero-copy parsing from bytes to strict commands.
3. **Engine (`engine.rs`)** – MemTable + WAL + SSTable management.

## 🛡️ Durability & Recovery

- **WAL replay** on boot restores all committed writes.
- **Tombstone-based deletion** avoids immediate disk rewrites.
- **Graceful shutdown** on SIGINT: flushes pending WAL, closes sockets cleanly.

## 📄 License

MIT