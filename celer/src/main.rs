#![allow(dead_code)]
mod ring_buffer;

use nix::cmsg_space;
use nix::sys::socket::{recvmsg, ControlMessageOwned, MsgFlags};
use memmap2::MmapMut;
use std::os::unix::io::{AsRawFd, FromRawFd};
use std::os::unix::net::UnixListener;
use std::fs::{remove_file, File};
use std::thread;

use warp::Filter;
use tokio::sync::broadcast;
// ESTA ES LA LÍNEA MÁGICA:
use futures_util::{SinkExt, StreamExt}; 
use serde::Serialize;

const FAST_PATH_SIZE: usize = 65536;
const IP_INDEX_MASK: u32 = (FAST_PATH_SIZE - 1) as u32;
const DDOS_THRESHOLD: u32 = 10_000;

#[derive(Clone, Copy)]
struct IpState {
    ip: u32,
    count: u32,
    blocked: bool,
}

// Estructura de la Alerta que viajará como JSON al Navegador
#[derive(Clone, Serialize)]
struct ThreatAlert {
    ip: String,
    threat_type: String,
    packets_seen: u32,
}

#[tokio::main]
async fn main() {
    // 1. EL PUENTE INTERNO (Sync -> Async)
    // Canal capaz de guardar hasta 100 alertas simultáneas sin bloquear el procesador
    let (tx, _) = broadcast::channel::<ThreatAlert>(100);
    let tx_core = tx.clone();

    // 2. EL HILO DEL NÚCLEO (Síncrono, Alta Frecuencia, 80+ Mpps)
    thread::spawn(move || {
        let socket_path = "/tmp/celer_bridge.sock";
        let _ = remove_file(socket_path);

        let listener = UnixListener::bind(socket_path).expect("Fallo al crear socket");
        println!("👁️‍🗨️ [CELER-CORE] Centinela activo. Escuchando en /tmp/celer_bridge.sock...");

        let (stream, _) = listener.accept().expect("Fallo al aceptar conexión de Aegis");
        
        let mut iov_buf = [0u8; 4];
        let mut iov = [std::io::IoSliceMut::new(&mut iov_buf)];
        let mut cmsg_buffer = cmsg_space!([std::os::unix::io::RawFd; 1]);

        // 1. Recibir el mensaje
        let msg = recvmsg::<()>(stream.as_raw_fd(), &mut iov, Some(&mut cmsg_buffer), MsgFlags::empty())
            .expect("Fallo al recibir el mensaje");

        let mut received_fd = -1;

        // 2. Extraer el FD (Desempaquetando el Result de .cmsgs())
        // Usamos .expect() para obtener el CmsgIterator real
        let cmsgs = msg.cmsgs().expect("❌ [CELER] Error al procesar mensajes de control del Kernel");
        
        for cmsg in cmsgs {
            if let ControlMessageOwned::ScmRights(fds) = cmsg {
                if let Some(&fd) = fds.first() {
                    received_fd = fd;
                    break;
                }
            }
        }

        if received_fd == -1 { 
            panic!("❌ [CELER] Error crítico: No se recibió el File Descriptor de Aegis."); 
        }

        // 3. Crear el archivo desde el FD
        let file = unsafe { File::from_raw_fd(received_fd) };
        let mut mmap = unsafe { MmapMut::map_mut(&file).unwrap() };

        let ring_ptr = mmap.as_mut_ptr() as *mut ring_buffer::SharedRing;
        let mut consumer = unsafe { ring_buffer::CelerConsumer::new(ring_ptr) };

        let mut fast_path: Vec<IpState> = vec![IpState { ip: 0, count: 0, blocked: false }; FAST_PATH_SIZE];

        println!("👁️‍🗨️ [CELER-CORE] CEREBRO PRIMITIVO ON. Esperando telemetría de ataque...");

        loop {
            if let Some(event) = consumer.pop() {
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
                        
                        println!("🚫 [ALERTA L7] SYN Flood neutralizado: {}", ip_str);
                        
                        // DISPARO AL FRONTEND: Enviamos el JSON por el canal
                        let alert = ThreatAlert {
                            ip: ip_str,
                            threat_type: "DDoS L7 (SYN Flood)".to_string(),
                            packets_seen: state.count,
                        };
                        let _ = tx_core.send(alert); // Ignoramos si no hay navegadores conectados
                    }
                }
            } else {
                std::hint::spin_loop();
            }
        }
    });

    // 3. EL SERVIDOR WEB Y WEBSOCKETS (Asíncrono, Puerto 3030)
    // Lee el archivo HTML y lo sirve en la ruta raíz "/"
    let html_content = include_str!("index.html");
    let index_route = warp::path::end().map(move || warp::reply::html(html_content));

    // Ruta de WebSockets "/ws"
    let tx_ws = tx.clone();
    let ws_route = warp::path("ws")
        .and(warp::ws())
        .and(warp::any().map(move || tx_ws.clone()))
        .map(|ws: warp::ws::Ws, tx: broadcast::Sender<ThreatAlert>| {
            ws.on_upgrade(move |socket| async move {
                let (mut ws_tx, _) = socket.split();
                let mut rx = tx.subscribe(); // Cada pestaña del navegador recibe su propia copia
                
                while let Ok(alert) = rx.recv().await {
                    let msg = serde_json::to_string(&alert).unwrap();
                    if ws_tx.send(warp::ws::Message::text(msg)).await.is_err() {
                        break; // Se desconectó el cliente
                    }
                }
            })
        });

    let routes = index_route.or(ws_route);

    println!("🌐 [RYŪ DASHBOARD] Servidor web iniciado. Abre http://127.0.0.1:3030 en tu navegador.");
    warp::serve(routes).run(([0, 0, 0, 0], 3030)).await;
}