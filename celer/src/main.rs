#![allow(dead_code)]
mod otel;

use shared_ipc::{CelerConsumer, SharedRing, ACTION_DOWN, ACTION_MOVE, ACTION_UP};


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

use opentelemetry::trace::{
    SpanContext, SpanId, TraceContextExt, TraceFlags, TraceId, TraceState,
};
use opentelemetry::Context as OtelContext;
use tracing_opentelemetry::OpenTelemetrySpanExt;

// --- CONFIGURACIÓN TÁCTICA ---
const CELER_PORT: u16 = 9030;
const SESSION_POOL_SIZE: usize = 65536;
const SESSION_MASK: usize = SESSION_POOL_SIZE - 1;
const SPEED_THRESHOLD: f32 = 8.5;

static EVENTS_PROCESSED: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy)]
struct SessionState {
    active: bool,
    last_x: f32, last_y: f32,
    last_dx: f32, last_dy: f32,
    last_timestamp: u64,
    smoothed_precision: f32, // <--- Nueva: Inercia de precisión
}
// Campos sincronizados con el Frontend RYŪ v2.2
#[derive(Clone, Serialize)]
struct PrecisionEvent {
    session_id: u32,
    precision: f32,
    speed: f32,
}

#[tokio::main]
async fn main() {
    let _otel_provider = otel::init_tracing("celer");
    tracing::info!(event_bytes = std::mem::size_of::<shared_ipc::StrokeEvent>(), "celer: stroke event size");

    // 1. Canal de Telemetría (Broadcast para múltiples pestañas de Chrome/Firefox)
    let (tx, _) = broadcast::channel::<PrecisionEvent>(2048);
    let tx_for_core = tx.clone();

    // 2. MONITOR DE RENDIMIENTO (Terminal HUD)
    std::thread::spawn(move || {
        let mut last_count = 0;
        let mut last_time = Instant::now();

        loop {
            std::thread::sleep(Duration::from_millis(250));
            let current_count = EVENTS_PROCESSED.load(Ordering::Relaxed);
            let current_time = Instant::now();
            let delta_events = current_count.saturating_sub(last_count);
            let delta_time = current_time.duration_since(last_time).as_secs_f64();

            if delta_time > 0.0 {
                // 🎯 CAMBIAMOS LA MATEMÁTICA: Ahora son Paquetes Por Segundo (Pps)
                let pps = delta_events as f64 / delta_time;

                let stdout = io::stdout();
                let mut handle = stdout.lock();

                write!(handle, "\x1B[2J\x1B[H").unwrap();
                writeln!(handle, "========================================").unwrap();
                writeln!(handle, "     🐉 RYŪ: PRECISION ANALYZER         ").unwrap();
                writeln!(handle, "========================================").unwrap();
                writeln!(handle, "Status:      \x1B[32mSTREAMING MODEL\x1B[0m").unwrap();
                // 🎯 CAMBIAMOS LA ETIQUETA A Pps
                writeln!(handle, "Throughput:  \x1B[32m{:>12.2} Pps\x1B[0m", pps).unwrap();
                writeln!(handle, "----------------------------------------").unwrap();
            }
            last_count = current_count;
            last_time = current_time;
        }
    });

    // 3. MOTOR CINEMÁTICO (Hot Loop IPC - Memoria Compartida)
    thread::spawn(move || {
        let socket_path = "/tmp/celer_bridge.sock";
        let _ = remove_file(socket_path);
        let listener = UnixListener::bind(socket_path).unwrap();

        tracing::info!(socket = socket_path, "celer: awaiting aegis handshake");
        let (stream, _) = listener.accept().expect("Fallo al conectar con AEGIS");

        let mut iov_buf = [0u8; 4];
        let mut iov = [io::IoSliceMut::new(&mut iov_buf)];
        let mut cmsg_buffer = cmsg_space!([std::os::unix::io::RawFd; 1]);
        let msg = recvmsg::<()>(stream.as_raw_fd(), &mut iov, Some(&mut cmsg_buffer), MsgFlags::empty()).unwrap();
        let mut received_fd = -1;
        if let Some(ControlMessageOwned::ScmRights(fds)) = msg.cmsgs().next() { received_fd = fds[0]; }

        let file = unsafe { File::from_raw_fd(received_fd) };
        let mmap = unsafe { MmapMut::map_mut(&file).unwrap() };
        let ring_ptr = mmap.as_ptr() as *mut SharedRing;
        let mut consumer = unsafe { CelerConsumer::new(ring_ptr) };

        let mut fsm_pool: Vec<SessionState> = vec![
            SessionState {
                active: false, last_x: 0.0, last_y: 0.0, last_dx: 0.0, last_dy: 0.0,
                last_timestamp: 0, smoothed_precision: 100.0,
            };
            SESSION_POOL_SIZE
        ];

        loop {
            if let Some(event) = consumer.pop() {
                EVENTS_PROCESSED.fetch_add(1, Ordering::Relaxed);

                // Rebuild OTel parent context from W3C wire format embedded
                // in the StrokeEvent. If either field is the invalid sentinel
                // ([0u8; N]), fall back to the current local context — this
                // covers microbench paths and any legacy producer that pushes
                // events without an active tracing span.
                let trace_id = TraceId::from_bytes(event.trace_id);
                let parent_span_id = SpanId::from_bytes(event.parent_span_id);

                let parent_ctx = if trace_id != TraceId::INVALID
                    && parent_span_id != SpanId::INVALID
                {
                    let parent_sc = SpanContext::new(
                        trace_id,
                        parent_span_id,
                        TraceFlags::SAMPLED,
                        true, // is_remote: crossed process boundary via shmem
                        TraceState::default(),
                    );
                    OtelContext::new().with_remote_span_context(parent_sc)
                } else {
                    OtelContext::current()
                };

                let span = tracing::info_span!(
                    "celer.process_stroke",
                    session_id = event.session_id,
                    action = event.action,
                );
                span.set_parent(parent_ctx);
                let _enter = span.enter();

                let state = &mut fsm_pool[(event.session_id as usize) & SESSION_MASK];

                match event.action {
                    ACTION_DOWN => {
                        // Inicio de trazo: arma la FSM para esta sesión.
                        state.active = true;
                        state.last_x = event.x;
                        state.last_y = event.y;
                        state.last_timestamp = event.timestamp;
                        state.smoothed_precision = 100.0;
                    }
                    ACTION_MOVE => {
                        if state.active {
                            let dx = event.x - state.last_x;
                            let dy = event.y - state.last_y;
                            let dist = dx.hypot(dy);

                            if dist > 0.0 {
                                let instant_precision = if dist > 5.0 { 95.0 } else { 45.0 };

                                let alpha = 0.15;
                                state.smoothed_precision =
                                    (instant_precision * alpha) + (state.smoothed_precision * (1.0 - alpha));


                                let _ = tx_for_core.send(PrecisionEvent {
                                    session_id: event.session_id,
                                    precision: state.smoothed_precision,
                                    speed: dist,
                                });

                                state.last_x = event.x;
                                state.last_y = event.y;
                            }
                        }
                    }
                    ACTION_UP => { state.active = false; }
                    _ => {}
                }
            } else {
                std::hint::spin_loop();
            }
        }
    });

    // 4. SERVIDOR DE TELEMETRÍA (Warp + WebSocket + CORS)
    let cors = warp::cors()
        .allow_any_origin()
        .allow_methods(vec!["GET", "POST", "DELETE"])
        .allow_headers(vec!["content-type"]);

    let ws_route = warp::path("ws")
        .and(warp::ws())
        .and(warp::any().map(move || tx.clone()))
        .map(|ws: warp::ws::Ws, tx: broadcast::Sender<PrecisionEvent>| {
            ws.on_upgrade(move |socket| async move {
                let (mut ws_tx, _) = socket.split();
                let mut rx = tx.subscribe();

                // 🛡️ BUCLE BLINDADO ANTI-SATURACIÓN
                loop {
                    match rx.recv().await {
                        Ok(event) => {
                            if let Ok(msg) = serde_json::to_string(&event) {
                                if ws_tx.send(warp::ws::Message::text(msg)).await.is_err() {
                                    break; // El navegador se cerró
                                }
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                            // 🚀 Si hay demasiados datos, ignoramos los viejos y seguimos
                            continue;
                        }
                        Err(_) => {
                            break; // El canal principal murió
                        }
                    }
                }
            })
        });

    // 🚨 RUTA DE PRUEBA (Para saber si Warp está vivo en el navegador)
    let ping_route = warp::path::end().map(|| "¡RYŪ DOJO: WARP ESTÁ VIVO!");

    let routes = ws_route.or(ping_route).with(cors);

    tracing::info!(port = 3031, "celer: ryuu http+ws listening");

    // MUDANZA AL PUERTO 3031
    warp::serve(routes)
        .run(([0, 0, 0, 0], 3031))
        .await;
}