//! Microbenchmark del ring buffer SPSC en aislamiento.
//!
//! No hay red, no hay mmap, no hay IPC entre procesos. Mide la velocidad
//! máxima del lock-free SPSC en memoria del mismo proceso.
//!
//! Uso:
//!   cargo run --release --example throughput -p shared-ipc
//!   cargo run --release --example throughput -p shared-ipc -- 100000000

use shared_ipc::{AegisProducer, CelerConsumer, SharedRing, StrokeEvent, RING_CAPACITY};
use std::alloc::{alloc_zeroed, dealloc, Layout};
use std::hint::black_box;
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::Instant;

const DEFAULT_N: usize = 50_000_000;
const WARMUP_N: usize = 1_000_000;

fn make_ring() -> *mut SharedRing {
    let layout = Layout::new::<SharedRing>();
    let ptr = unsafe { alloc_zeroed(layout) as *mut SharedRing };
    assert!(!ptr.is_null(), "alloc_zeroed failed");
    ptr
}

fn free_ring(ptr: *mut SharedRing) {
    unsafe { dealloc(ptr as *mut u8, Layout::new::<SharedRing>()) };
}

fn make_event(i: u64) -> StrokeEvent {
    StrokeEvent {
        session_id: 1,
        timestamp: i,
        x: 0.0,
        y: 0.0,
        pressure: 1.0,
        action: 1,
        trace_id: [0u8; 16], // microbench sin OTel: sentinel W3C "no trace"
    }
}

fn bench_single_threaded(n: usize) {
    let ptr = make_ring();
    let mut producer = unsafe { AegisProducer::new(ptr) };
    let mut consumer = unsafe { CelerConsumer::new(ptr) };

    // Warmup para que branch predictor, TLB y caches se acomoden.
    for i in 0..WARMUP_N as u64 {
        producer.push(make_event(i)).unwrap();
        let _ = consumer.pop().unwrap();
    }

    let start = Instant::now();
    let mut checksum: u64 = 0;
    for i in 0..n as u64 {
        let event = black_box(make_event(i));
        producer.push(event).unwrap();
        // black_box sobre el StrokeEvent completo previene que LLVM
        // prune la lectura de los campos no usados del payload.
        let evt = black_box(consumer.pop().unwrap());
        checksum = checksum.wrapping_add(evt.timestamp);
    }
    let elapsed = start.elapsed();
    black_box(checksum);

    let pairs_per_sec = n as f64 / elapsed.as_secs_f64();
    let ns_per_pair = elapsed.as_nanos() as f64 / n as f64;

    println!("--- single-threaded (push+pop alternado, sin contención) ---");
    println!("  iteraciones:    {}", n);
    println!("  tiempo:         {:.3} s", elapsed.as_secs_f64());
    println!("  throughput:     {:.2}M pares push+pop / s", pairs_per_sec / 1_000_000.0);
    println!("  latencia:       {:.2} ns / par push+pop", ns_per_pair);

    free_ring(ptr);
}

fn bench_spsc(n: usize) {
    let ptr = make_ring();
    let producer = unsafe { AegisProducer::new(ptr) };
    let consumer = unsafe { CelerConsumer::new(ptr) };

    let barrier = Arc::new(Barrier::new(2));
    let n_total = n;

    let p_barrier = barrier.clone();
    let producer_handle = thread::spawn(move || {
        let mut producer = producer;
        p_barrier.wait();
        let mut sent = 0u64;
        while sent < n_total as u64 {
            let event = black_box(make_event(sent));
            while producer.push(event).is_err() {
                std::hint::spin_loop();
            }
            sent += 1;
        }
    });

    let c_barrier = barrier.clone();
    let consumer_handle = thread::spawn(move || {
        let mut consumer = consumer;
        c_barrier.wait();
        let start = Instant::now();
        let mut received = 0u64;
        let mut checksum: u64 = 0;
        while received < n_total as u64 {
            if let Some(evt) = consumer.pop() {
                let evt = black_box(evt);
                checksum = checksum.wrapping_add(evt.timestamp);
                received += 1;
            } else {
                std::hint::spin_loop();
            }
        }
        let elapsed = start.elapsed();
        black_box(checksum);
        elapsed
    });

    producer_handle.join().expect("producer panicked");
    let elapsed = consumer_handle.join().expect("consumer panicked");

    let ops_per_sec = n as f64 / elapsed.as_secs_f64();
    let ns_per_op = elapsed.as_nanos() as f64 / n as f64;

    println!("\n--- SPSC dos threads (productor || consumidor) ---");
    println!("  eventos:        {}", n);
    println!("  tiempo:         {:.3} s", elapsed.as_secs_f64());
    println!("  throughput:     {:.2}M eventos / s", ops_per_sec / 1_000_000.0);
    println!("  latencia media: {:.2} ns / evento end-to-end", ns_per_op);

    free_ring(ptr);
}

fn main() {
    let n: usize = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_N);

    println!("=== shared-ipc :: microbenchmark del ring SPSC ===");
    println!("config:  N={}, RING_CAPACITY={}", n, RING_CAPACITY);
    println!("modo:    {}", if cfg!(debug_assertions) {
        "DEBUG (los números van a ser basura; relanzar con --release)"
    } else {
        "release"
    });
    println!();

    bench_single_threaded(n);
    bench_spsc(n);

    println!();
    println!("Para más volumen: cargo run --release --example throughput -p shared-ipc -- 100000000");
    println!("Para reducir ruido: cerrar Chrome, evitar carga térmica, correr 2-3 veces y tomar la mediana.");
}