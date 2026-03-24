#![allow(dead_code)]
mod ring_buffer;

use nix::cmsg_space;
use nix::sys::socket::{recvmsg, ControlMessageOwned, MsgFlags};
use memmap2::MmapMut;
use std::os::unix::io::{AsRawFd, FromRawFd};
use std::os::unix::net::UnixListener;
use std::fs::{remove_file, File};
use std::thread;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Instant, Duration};
use std::io::{self, Write};

use warp::Filter;
use tokio::sync::broadcast;
use futures_util::{SinkExt, StreamExt}; 
use serde::Serialize;

// ... (Tus imports arriba siguen igual)

// --- CONFIGURACIÓN ---
const FAST_PATH_SIZE: usize = 65536;
const IP_INDEX_MASK: u32 = (FAST_PATH_SIZE - 1) as u32;
const DDOS_THRESHOLD: u32 = 10_000;

// 1. EL CONTADOR ATÓMICO (Fuera del main)
static EVENTS_PROCESSED: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy)]
struct IpState {
    ip: u32,
    count: u32,
    blocked: bool,
}

#[derive(Clone, Serialize)]
struct ThreatAlert {
    ip: String,
    threat_type: String,
    packets_seen: u32,
}

#[tokio::main]
async fn main() {
    // 2. CANAL PARA EL DASHBOARD RYŪ
    let (tx, _) = broadcast::channel::<ThreatAlert>(100);
    let tx_for_core = tx.clone(); 

    // 3. LANZAR HILO DE TELEMETRÍA (El Observador)
    // Se lanza primero para que esté listo cuando empiece el tráfico.
    std::thread::spawn(move || {
        let mut last_count = 0;
        let mut last_time = Instant::now();
        let mut peak_mpps = 0.0;
        let stdout = io::stdout();
        let mut handle = stdout.lock();

        write!(handle, "\x1B[2J\x1B[H").unwrap(); // Limpiar pantalla

        loop {
            std::thread::sleep(Duration::from_millis(200)); 

            let current_count = EVENTS_PROCESSED.load(Ordering::Relaxed);
            let current_time = Instant::now();
            let delta_events = current_count.saturating_sub(last_count);
            let delta_time = current_time.duration_since(last_time).as_secs_f64();
            
            if delta_time > 0.0 {
                let current_mpps = (delta_events as f64 / delta_time) / 1_000_000.0;
                if current_mpps > peak_mpps { peak_mpps = current_mpps; }

                write!(handle, "\x1B[H").unwrap(); 
                writeln!(handle, "========================================").unwrap();
                writeln!(handle, "     🛰️  VANGUARD TELEMETRY (LIVE)     ").unwrap();
                writeln!(handle, "========================================").unwrap();
                writeln!(handle, "Status:      \x1B[32mRUNNING\x1B[0m").unwrap();
                writeln!(handle, "Total Count: \x1B[36m{:>12} events\x1B[0m", current_count).unwrap();
                let color = if current_mpps > 0.1 { "\x1B[32m" } else { "\x1B[37m" };
                writeln!(handle, "Inst. Speed: {}{:>12.2} Mpps\x1B[0m", color, current_mpps).unwrap();
                writeln!(handle, "PEAK SPEED:  \x1B[33m\x1B[1m{:>12.2} Mpps\x1B[0m", peak_mpps).unwrap();
                writeln!(handle, "----------------------------------------").unwrap();
                handle.flush().unwrap();
            }
            last_count = current_count;
            last_time = last_time + Duration::from_secs_f64(delta_time);
        }
    });

    // 4. EL HILO DE INFRAESTRUCTURA Y HOT LOOP (El Motor)
    thread::spawn(move || {
        let socket_path = "/tmp/celer_bridge.sock";
        let _ = remove_file(socket_path);
        let listener = UnixListener::bind(socket_path).expect("Error socket");
        let (stream, _) = listener.accept().expect("Error conexión Aegis");
        
        // --- Setup de Memoria Compartida ---
        let mut iov_buf = [0u8; 4];
        let mut iov = [io::IoSliceMut::new(&mut iov_buf)];
        let mut cmsg_buffer = cmsg_space!([std::os::unix::io::RawFd; 1]);
        let msg = recvmsg::<()>(stream.as_raw_fd(), &mut iov, Some(&mut cmsg_buffer), MsgFlags::empty()).unwrap();
        
        let mut received_fd = -1;
        if let Some(cmsg) = msg.cmsgs().unwrap().next() {
            if let ControlMessageOwned::ScmRights(fds) = cmsg {
                received_fd = fds[0];
            }
        }

        let file = unsafe { File::from_raw_fd(received_fd) };
        let mut mmap = unsafe { MmapMut::map_mut(&file).unwrap() };
        let ring_ptr = mmap.as_mut_ptr() as *mut ring_buffer::SharedRing;
        let mut consumer = unsafe { ring_buffer::CelerConsumer::new(ring_ptr) };
        let mut fast_path: Vec<IpState> = vec![IpState { ip: 0, count: 0, blocked: false }; FAST_PATH_SIZE];

        // --- HOT LOOP ---
        loop {
            if let Some(event) = consumer.pop() {
                EVENTS_PROCESSED.fetch_add(1, Ordering::Relaxed);

                let index = (event.source_ip & IP_INDEX_MASK) as usize;
                let state = &mut fast_path[index];

                if state.ip != event.source_ip {
                    state.ip = event.source_ip;
                    state.count = 1;
                    state.blocked = false;
                } else {
                    state.count += 1;
                    if state.count > DDOS_THRESHOLD && !state.blocked {
                        state.blocked = true;
                        
                        let b1 = (event.source_ip >> 24) as u8;
                        let b2 = (event.source_ip >> 16) as u8;
                        let b3 = (event.source_ip >> 8) as u8;
                        let b4 = event.source_ip as u8;
                        let ip_str = format!("{}.{}.{}.{}", b1, b2, b3, b4);
                        
                        let _ = tx_for_core.send(ThreatAlert {
                            ip: ip_str,
                            threat_type: "DDoS L7 (SYN Flood)".to_string(),
                            packets_seen: state.count,
                        }); 
                    }
                }
            } else {
                std::hint::spin_loop(); 
            }
        }
    });

    // 5. SERVIDOR RYŪ (Dashboard Web)
    let html_content = include_str!("index.html");
    let index_route = warp::path::end().map(move || warp::reply::html(html_content));

    let tx_ws = tx.clone();
    let ws_route = warp::path("ws")
        .and(warp::ws())
        .and(warp::any().map(move || tx_ws.clone()))
        .map(|ws: warp::ws::Ws, tx: broadcast::Sender<ThreatAlert>| {
            ws.on_upgrade(move |socket| async move {
                let (mut ws_tx, _) = socket.split();
                let mut rx = tx.subscribe();
                while let Ok(alert) = rx.recv().await {
                    let msg = serde_json::to_string(&alert).unwrap();
                    if ws_tx.send(warp::ws::Message::text(msg)).await.is_err() { break; }
                }
            })
        });

    warp::serve(index_route.or(ws_route)).run(([0, 0, 0, 0], 3030)).await;
}