#![allow(dead_code)]
mod lexer;
mod ast; // <-- Agregá esta línea
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
use serde::Serialize;
use futures_util::{SinkExt, StreamExt}; 

// --- CONFIGURACIÓN TÁCTICA ---
const CELER_PORT: u16 = 9030;
const SESSION_POOL_SIZE: usize = 65536;
const SESSION_MASK: usize = SESSION_POOL_SIZE - 1;
const SPEED_THRESHOLD: f32 = 8.5;       

const ACTION_DOWN: u8 = 0;
const ACTION_MOVE: u8 = 1;
const ACTION_UP: u8 = 2;

static EVENTS_PROCESSED: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy)]
struct SessionState {
    active: bool,
    last_x: f32, last_y: f32,
    last_dx: f32, last_dy: f32,
    last_timestamp: u64,
}

// Campos sincronizados con el Frontend RYŪ v2.2
#[derive(Clone, Serialize)]
struct PrecisionEvent {
    session_id: u32,
    precision: f32,      
    feedback: String,    
    speed: f32,
}

#[tokio::main]
async fn main() {
    // Canal para transmitir el juicio de Celer hacia el WebSocket
    let (tx, _) = broadcast::channel::<PrecisionEvent>(2048);
    let tx_for_core = tx.clone(); 

    // 1. MONITOR DE RENDIMIENTO (Terminal)
    std::thread::spawn(move || {
        let mut last_count = 0;
        let mut last_time = Instant::now();
        let stdout = io::stdout();
        let mut handle = stdout.lock();
        write!(handle, "\x1B[2J\x1B[H").unwrap(); 
        loop {
            std::thread::sleep(Duration::from_millis(250)); 
            let current_count = EVENTS_PROCESSED.load(Ordering::Relaxed);
            let current_time = Instant::now();
            let delta_events = current_count.saturating_sub(last_count);
            let delta_time = current_time.duration_since(last_time).as_secs_f64();
            if delta_time > 0.0 {
                let mpps = (delta_events as f64 / delta_time) / 1_000_000.0;
                write!(handle, "\x1B[H").unwrap(); 
                writeln!(handle, "========================================").unwrap();
                writeln!(handle, "     🐉 RYŪ: PRECISION ANALYZER         ").unwrap();
                writeln!(handle, "========================================").unwrap();
                writeln!(handle, "Status:      \x1B[32mSTREAMING MODEL\x1B[0m").unwrap();
                writeln!(handle, "Throughput:  \x1B[32m{:>12.2} Mpps\x1B[0m", mpps).unwrap();
                writeln!(handle, "----------------------------------------").unwrap();
                handle.flush().unwrap();
            }
            last_count = current_count;
            last_time = current_time;
        }
    });

    // 2. MOTOR CINEMÁTICO (Hot Loop IPC)
    thread::spawn(move || {
        let socket_path = "/tmp/celer_bridge.sock";
        let _ = remove_file(socket_path);
        let listener = UnixListener::bind(socket_path).unwrap();
        
        // El servidor web arrancará, pero este hilo esperará a AEGIS
        let (stream, _) = listener.accept().expect("Fallo al conectar con AEGIS");
        
        let mut iov_buf = [0u8; 4];
        let mut iov = [io::IoSliceMut::new(&mut iov_buf)];
        let mut cmsg_buffer = cmsg_space!([std::os::unix::io::RawFd; 1]);
        let msg = recvmsg::<()>(stream.as_raw_fd(), &mut iov, Some(&mut cmsg_buffer), MsgFlags::empty()).unwrap();
        let mut received_fd = -1;
        if let Some(ControlMessageOwned::ScmRights(fds)) = msg.cmsgs().next() { received_fd = fds[0]; }

        let file = unsafe { File::from_raw_fd(received_fd) };
        let mut mmap = unsafe { MmapMut::map_mut(&file).unwrap() };
        let ring_ptr = mmap.as_mut_ptr() as *mut ring_buffer::SharedRing;
        let mut consumer = unsafe { ring_buffer::CelerConsumer::new(ring_ptr) };

        let mut fsm_pool = vec![SessionState { active: false, last_x: 0.0, last_y: 0.0, last_dx: 0.0, last_dy: 0.0, last_timestamp: 0 }; SESSION_POOL_SIZE];

        loop {
            if let Some(event) = consumer.pop() {
                EVENTS_PROCESSED.fetch_add(1, Ordering::Relaxed);
                let state = &mut fsm_pool[(event.session_id as usize) & SESSION_MASK];

                match event.action {
                    ACTION_DOWN => {
                        *state = SessionState { active: true, last_x: event.x, last_y: event.y, last_dx: 0.0, last_dy: 0.0, last_timestamp: event.timestamp };
                    }
                    ACTION_MOVE => {
                        if state.active {
                            let dx = event.x - state.last_x;
                            let dy = event.y - state.last_y;
                            let dist = dx.hypot(dy);
                            let dt = event.timestamp.saturating_sub(state.last_timestamp);

                            if dist > 0.6 && dt > 0 {
                                let speed = dist / (dt as f32);
                                let nx = dx / dist; 
                                let ny = dy / dist;

                                let mut precision = 100.0;
                                let mut feedback = "Trazo Maestro".to_string();

                                if state.last_dx != 0.0 {
                                    let dot = (nx * state.last_dx) + (ny * state.last_dy);
                                    precision = ((dot + 1.0) / 2.0) * 100.0;
                                    
                                    feedback = match precision {
                                        p if p > 94.0 => "🎯 Alineación Perfecta".to_string(),
                                        p if p > 75.0 => "🟢 Estabilidad Óptima".to_string(),
                                        p if p > 55.0 => "🟡 Pulso Inestable".to_string(),
                                        _ => "🔴 Error de Precisión".to_string(),
                                    };
                                }

                                if speed > SPEED_THRESHOLD {
                                    feedback = "⚠️ Velocidad Excesiva".to_string();
                                    precision = (precision - 15.0).max(0.0);
                                }

                                let _ = tx_for_core.send(PrecisionEvent {
                                    session_id: event.session_id,
                                    precision,
                                    feedback,
                                    speed,
                                });

                                state.last_dx = nx; state.last_dy = ny;
                                state.last_x = event.x; state.last_y = event.y;
                                state.last_timestamp = event.timestamp;
                            }
                        }
                    }
                    ACTION_UP => { state.active = false; }
                    _ => {}
                }
            } else { std::hint::spin_loop(); }
        }
    });

    // 3. SERVIDOR RYŪ (WEB + WEBSOCKET)
    let html_content = include_str!("index.html");

    // Servir el HTML en la raíz
    let index_route = warp::get()
        .and(warp::path::end())
        .map(move || warp::reply::html(html_content));

    // Servir el WebSocket de Telemetría
    let tx_ws = tx.clone();
    let ws_route = warp::path("ws")
        .and(warp::ws())
        .and(warp::any().map(move || tx_ws.clone()))
        .map(|ws: warp::ws::Ws, tx: broadcast::Sender<PrecisionEvent>| {
            ws.on_upgrade(move |socket| async move {
                let (mut ws_tx, _) = socket.split();
                let mut rx = tx.subscribe();
                while let Ok(event) = rx.recv().await {
                    let msg = serde_json::to_string(&event).unwrap();
                    if ws_tx.send(warp::ws::Message::text(msg)).await.is_err() { break; }
                }
            })
        });

    let routes = index_route.or(ws_route);

    println!("🚀 RYŪ DOJO ONLINE | URL: http://localhost:3030");
    // Bind global en 0.0.0.0 para que Firefox no falle
    warp::serve(routes).run(([0, 0, 0, 0], CELER_PORT)).await;
}