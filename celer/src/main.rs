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

// --- CONFIGURACIÓN ---
const FAST_PATH_SIZE: usize = 65536;
const IP_INDEX_MASK: u32 = (FAST_PATH_SIZE - 1) as u32;
const DDOS_THRESHOLD: u32 = 10_000;

// Contador global para el hilo de métricas
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
    // 1. CANAL CENTRAL (El único que importa)
    let (tx, _) = broadcast::channel::<ThreatAlert>(100);
    let tx_for_core = tx.clone(); // Clonamos para enviarlo al hilo de procesamiento

    // 2. EL HILO DE INFRAESTRUCTURA (Socket Unix + Memmap)
    thread::spawn(move || {
        let socket_path = "/tmp/celer_bridge.sock";
        let _ = remove_file(socket_path);

        let listener = UnixListener::bind(socket_path).expect("Fallo al crear socket");
        
        // --- HILO DE TELEMETRÍA (Dashboard de Terminal) ---
        std::thread::spawn(move || {
        let mut last_count = 0;
        let mut last_time = Instant::now();
        let mut peak_mpps = 0.0; // <--- Guardamos el récord
        let stdout = io::stdout();
        let mut handle = stdout.lock();

        write!(handle, "\x1B[2J\x1B[H").unwrap();
        writeln!(handle, "========================================").unwrap();
        writeln!(handle, "     🛰️  VANGUARD TELEMETRY (LIVE)     ").unwrap();
        writeln!(handle, "========================================").unwrap();
        writeln!(handle, "Status: \x1B[32mRUNNING\x1B[0m").unwrap();
        writeln!(handle, "Total Events: ").unwrap();
        writeln!(handle, "Throughput:   ").unwrap();
        writeln!(handle, "Peak Speed:   ").unwrap(); // <--- Nueva línea
        writeln!(handle, "----------------------------------------").unwrap();
        handle.flush().unwrap();

        loop {
            // Bajamos el sleep a 500ms para tener más chances de capturar la ráfaga
            std::thread::sleep(Duration::from_millis(500));

            let current_count = EVENTS_PROCESSED.load(Ordering::Relaxed);
            let current_time = Instant::now();
            let events_delta = current_count.saturating_sub(last_count);
            let duration = current_time.duration_since(last_time).as_secs_f64();
            let mpps = (events_delta as f64 / duration) / 1_000_000.0;

            if mpps > peak_mpps { peak_mpps = mpps; }

            // Sobrescribimos las 4 líneas
            write!(handle, "\x1B[4A").unwrap(); 
            writeln!(handle, "Total Events: \x1B[0K{} events", current_count).unwrap();
            
            let mpps_color = if mpps > 0.1 { "\x1B[32m" } else { "\x1B[37m" };
            writeln!(handle, "Throughput:   \x1B[0K{}{:.2} Mpps\x1B[0m", mpps_color, mpps).unwrap();
            
            // Mostramos el récord en Amarillo
            writeln!(handle, "Peak Speed:   \x1B[0K\x1B[33m{:.2} Mpps\x1B[0m", peak_mpps).unwrap();
            
            write!(handle, "\x1B[1B").unwrap(); 
            handle.flush().unwrap();

            last_count = current_count;
            last_time = current_time;
            }
        });

        // Esperar conexión de Aegis
        let (stream, _) = listener.accept().expect("Fallo al aceptar conexión de Aegis");
        let mut iov_buf = [0u8; 4];
        let mut iov = [io::IoSliceMut::new(&mut iov_buf)];
        let mut cmsg_buffer = cmsg_space!([std::os::unix::io::RawFd; 1]);

        let msg = recvmsg::<()>(stream.as_raw_fd(), &mut iov, Some(&mut cmsg_buffer), MsgFlags::empty()).unwrap();
        let mut received_fd = -1;
        let cmsgs = msg.cmsgs().expect("Error en cmsgs");
        for cmsg in cmsgs {
            if let ControlMessageOwned::ScmRights(fds) = cmsg {
                if let Some(&fd) = fds.first() { received_fd = fd; break; }
            }
        }

        let file = unsafe { File::from_raw_fd(received_fd) };
        let mut mmap = unsafe { MmapMut::map_mut(&file).unwrap() };
        let ring_ptr = mmap.as_mut_ptr() as *mut ring_buffer::SharedRing;
        let mut consumer = unsafe { ring_buffer::CelerConsumer::new(ring_ptr) };

        // --- HILO DEL NÚCLEO (Celer Core - Hot Loop) ---
        let mut fast_path: Vec<IpState> = vec![IpState { ip: 0, count: 0, blocked: false }; FAST_PATH_SIZE];
        
        loop {
            if let Some(event) = consumer.pop() {
                // 1. Actualizar Métricas
                EVENTS_PROCESSED.fetch_add(1, Ordering::Relaxed);

                // 2. Lógica de Detección DDoS
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
                        
                        // 3. ENVIAR ALERTA REAL AL DASHBOARD
                        let alert = ThreatAlert {
                            ip: ip_str,
                            threat_type: "DDoS L7 (SYN Flood)".to_string(),
                            packets_seen: state.count,
                        };
                        let _ = tx_for_core.send(alert); 
                    }
                }
            } else {
                thread::yield_now(); // No frenamos el core, solo cedemos un turno
            }
        }
    });

    // 3. SERVIDOR WEB Y WEBSOCKETS
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

    let routes = index_route.or(ws_route);
    warp::serve(routes).run(([0, 0, 0, 0], 3030)).await;
}