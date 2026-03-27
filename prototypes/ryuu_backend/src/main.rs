use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::sync::mpsc;
use tokio::net::TcpListener;
use tokio_tungstenite::{accept_async, tungstenite::Message};

// --- Mensajes internos ---
#[derive(Debug, Clone)]
struct StrokePoint {
    x: f32,
    y: f32,
    flag: u8,   // 1=start, 0=move, 2=end
    session_id: u64,
}

// --- Telemetría JSON ---
#[derive(Serialize, Deserialize, Clone)]
struct Telemetry {
    flow_score: f32,
    threat_type: String,
}

// --- Estado de una sesión ---
struct Session {
    last_point: Option<(f32, f32)>,
    last_time: Option<std::time::Instant>,
    velocities: Vec<f32>,
    last_direction: Option<(f32, f32)>,
    stroke_count: u32,
    expected_stroke: u8,
}

impl Session {
    fn new() -> Self {
        Self {
            last_point: None,
            last_time: None,
            velocities: Vec::with_capacity(10),
            last_direction: None,
            stroke_count: 0,
            expected_stroke: 1,
        }
    }

    fn process_point(&mut self, point: &StrokePoint) -> (f32, Option<String>) {
        let mut score: f32 = 100.0;
        let mut anomaly = None;

        match point.flag {
            1 => { // inicio de trazo
                self.last_point = Some((point.x, point.y));
                self.last_time = Some(std::time::Instant::now());
                self.stroke_count += 1;
                // Validación simple de orden de trazos (ejemplo)
                if self.stroke_count == 1 && self.expected_stroke != 1 {
                    anomaly = Some("WRONG_STROKE_ORDER".to_string());
                    score -= 40.0;
                } else if self.stroke_count == 2 && self.expected_stroke != 2 {
                    anomaly = Some("WRONG_STROKE_ORDER".to_string());
                    score -= 40.0;
                }
                self.expected_stroke += 1;
            }
            0 => { // movimiento
                if let Some((lx, ly)) = self.last_point {
                    let dx = point.x - lx;
                    let dy = point.y - ly;
                    let distance = (dx*dx + dy*dy).sqrt();
                    if let Some(last_time) = self.last_time {
                        let dt = last_time.elapsed().as_secs_f32();
                        if dt > 0.0 {
                            let speed = distance / dt;
                            if speed > 2.0 {
                                score -= 20.0;
                                anomaly = Some("EXCESSIVE_SPEED".to_string());
                            }
                            self.velocities.push(speed);
                            if self.velocities.len() > 10 {
                                self.velocities.remove(0);
                            }
                            if self.velocities.len() >= 5 {
                                let mean = self.velocities.iter().sum::<f32>() / self.velocities.len() as f32;
                                let variance = self.velocities.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / self.velocities.len() as f32;
                                let jitter = variance.sqrt();
                                if jitter > 0.5 {
                                    score -= 15.0;
                                    anomaly = Some("ERRATIC_JITTER".to_string());
                                }
                            }
                            if let Some((ldx, ldy)) = self.last_direction {
                                let dot = dx*ldx + dy*ldy;
                                let norm = distance * (ldx*ldx + ldy*ldy).sqrt();
                                if norm > 1e-6 {
                                    let angle_cos = dot / norm;
                                    if angle_cos < 0.3 { // cambio brusco
                                        score -= 20.0;
                                        anomaly = Some("SHARP_DIRECTION_CHANGE".to_string());
                                    }
                                }
                            }
                            self.last_direction = Some((dx, dy));
                        }
                    }
                }
                self.last_point = Some((point.x, point.y));
                self.last_time = Some(std::time::Instant::now());
            }
            2 => { // fin de trazo
                self.last_point = None;
                self.last_time = None;
                self.last_direction = None;
            }
            _ => {}
        }

        let score = score.clamp(0.0, 100.0);
        (score, anomaly)
    }

    fn reset(&mut self) {
        *self = Self::new();
    }
}

// --- Tarea que procesa puntos y envía telemetría ---
async fn celer_task(
    mut rx: mpsc::Receiver<StrokePoint>,
    tx_telemetry: tokio::sync::broadcast::Sender<Telemetry>,
) {
    let mut sessions: HashMap<u64, Session> = HashMap::new();

    while let Some(point) = rx.recv().await {
        let session = sessions.entry(point.session_id).or_insert_with(Session::new);
        let (score, anomaly) = session.process_point(&point);
        let threat = anomaly.unwrap_or_else(|| "STABLE".to_string());

        let telemetry = Telemetry {
            flow_score: score,
            threat_type: threat,
        };
        // Enviar telemetría a todos los clientes WebSocket de CELER
        let _ = tx_telemetry.send(telemetry);
    }
}

// --- Servidor AEGIS (WebSocket binario) ---
async fn aegis_server(
    listener: TcpListener,
    tx_stroke: mpsc::Sender<StrokePoint>,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("AEGIS WebSocket listening on ws://localhost:9090");

    let mut next_session_id = 0u64;

    while let Ok((stream, _)) = listener.accept().await {
        let peer_addr = stream.peer_addr()?;
        println!("New AEGIS connection from {}", peer_addr);

        let tx_stroke = tx_stroke.clone();
        let session_id = next_session_id;
        next_session_id += 1;

        tokio::spawn(async move {
            let ws_stream = accept_async(stream).await.unwrap();
            let (mut _ws_sender, mut ws_receiver) = ws_stream.split();

            // No enviamos nada, solo recibimos binario
            while let Some(msg) = ws_receiver.next().await {
                match msg {
                    Ok(Message::Binary(data)) if data.len() == 13 => {
                        let x = f32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                        let y = f32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                        let _pressure = f32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                        let flag = data[12];
                        let point = StrokePoint { x, y, flag, session_id };
                        if let Err(e) = tx_stroke.send(point).await {
                            eprintln!("Error sending to CELER: {}", e);
                            break;
                        }
                    }
                    Ok(Message::Binary(_)) => {
                        // ignorar otros tamaños
                    }
                    Ok(Message::Close(_)) => break,
                    _ => {}
                }
            }
        });
    }
    Ok(())
}

// --- Servidor CELER (WebSocket JSON) ---
async fn celer_websocket_server(
    listener: TcpListener,
    mut rx_telemetry: tokio::sync::broadcast::Receiver<Telemetry>,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("CELER WebSocket listening on ws://localhost:9030");

    while let Ok((stream, _)) = listener.accept().await {
        let peer_addr = stream.peer_addr()?;
        println!("New CELER connection from {}", peer_addr);

        let mut rx = rx_telemetry.resubscribe();
        tokio::spawn(async move {
            let ws_stream = match accept_async(stream).await {
                Ok(ws) => ws,
                Err(e) => {
                    eprintln!("Conexión rechazada (No es WebSocket): {}", e);
                    return;
                }
            };
            let (mut _ws_sender, mut ws_receiver) = ws_stream.split();
            while let Ok(telemetry) = rx.recv().await {
                let json = serde_json::to_string(&telemetry).unwrap();
                if ws_sender.send(Message::Text(json.into())).await.is_err() {
                    break; // cliente desconectado
                }
            }
        });
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Canales
    let (tx_stroke, rx_stroke) = mpsc::channel::<StrokePoint>(1000);
    let (tx_telemetry, _) = tokio::sync::broadcast::channel::<Telemetry>(100);

    // Lanzar tarea CELER
    tokio::spawn(celer_task(rx_stroke, tx_telemetry.clone()));

    // Servidores WebSocket
    let aegis_listener = TcpListener::bind("127.0.0.1:9090").await?;
    let celer_listener = TcpListener::bind("127.0.0.1:9030").await?;

    let aegis = aegis_server(aegis_listener, tx_stroke);
    let celer = celer_websocket_server(celer_listener, tx_telemetry.subscribe());

    tokio::select! {
        res = aegis => res?,
        res = celer => res?,
    }

    Ok(())
}