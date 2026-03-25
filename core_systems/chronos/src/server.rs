// Los BRAZOS (El Servidor TCP)
// ESTE ARCHIVO se encargará de escuchar conexiones, manejar la red y crear hilos (threads).
// NO sabe como GUARDAR datos. Simplemente le pide al ENGINE que lo haga!!!
// Es decir, tiene funciones especificas que luego POSTERIORMENTE solicitara al ENGINE (el cerebro)
// QUE las GUARDE en sus datos.

// Nota: En la Semana 10 DIA 1, blindamos este servidor para que registre las IP's de los usuarios que entran y salen
// Y lo protegemos para que si el usuario desconecta la computadora de forma repentina, el hilo muera en paz sin tumbar el server.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, RwLock};
use std::thread;

// Importamos el motor que acabamos de crear
use crate::engine::Engine;
use crate::parser::{self, Command}; // <---- IMPORTAMOS NUESTRO PARSER

// Creamos un tipo de dato público para que sea fácil de escribir
pub type Db = Arc<RwLock<Engine>>;

pub fn start_server(db: Db) {
    let listener = TcpListener::bind("127.0.0.1:8080").unwrap();
    println!("🚀 CHRONOS SERVER LISTO Y ESCUCHANDO EN TCP 127.0.0.1:8080");
    println!("   Esperando conexiones entrantes...\n");

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let db_clone = Arc::clone(&db);
                // Obtenemos la IP del cliente para nuestros logs
                let peer_addr = stream.peer_addr().unwrap();
                println!("   🟢 NUEVA CONEXIÓN: {}", peer_addr);

                thread::spawn(move || {
                    handle_client(stream, db_clone, peer_addr.to_string());
                });
            }
            Err(e) => println!("   ❌ Error de conexión entrante: {}", e),
        }
    }
}

// Esta función es privada (no tiene pub) porque solo se usa dentro de este archivo
fn handle_client(mut stream: TcpStream, db: Db, peer_addr: String) {
    let mut buffer = [0; 512];
    loop {
        match stream.read(&mut buffer) {
            Ok(bytes_read) => {
                // Si leemos 0 bytes, significa que el cliente cerró la conexión (EOF)
                if bytes_read == 0 {
                    println!("   🔴 DESCONECTADO (Limpio): {}", peer_addr);
                    break;
                }

                let raw_msg = String::from_utf8_lossy(&buffer[..bytes_read]);

                // 1. LE PASAMOS EL TEXTO SUCIO A NUESTRO PARSER
                let command = parser::parse(&raw_msg);

                // 2. EJECUTAMOS EL COMANDO TIPADO
                let response = match command {
                    Command::Set(key, value) => {
                        let mut engine = db.write().unwrap();
                        match engine.set(&key, &value) {
                            Ok(_) => "OK\n".to_string(),
                            Err(e) => format!("ERR {}\n", e),
                        }
                    }
                    Command::Del(key) => {
                        let mut engine = db.write().unwrap();
                        // Insertamos la lápida usando el mismo motor SET
                        match engine.set(&key, "__TOMBSTONE__") {
                            Ok(_) => "OK_DELETED\n".to_string(),
                            Err(e) => format!("ERR {}\n", e),
                        }
                    }
                    Command::Get(key) => {
                        let engine = db.read().unwrap();
                        match engine.get(&key) {
                            Some(v) if v == "__TOMBSTONE__" => "NULL\n".to_string(), // Fingimos demencia
                            Some(v) => format!("{}\n", v),
                            None => "NULL\n".to_string(),
                        }
                    }
                    Command::Compact => {
                        let mut engine = db.write().unwrap();
                        match engine.compact() {
                            Ok(_) => "OK_COMPACTED\n".to_string(),
                            Err(e) => format!("ERR_COMPACT {}\n", e),
                        }
                    }
                    Command::Ping => "PONG\n".to_string(),
                    Command::Unknown => "ERR_UNKNOWN_COMMAND\n".to_string(),
                };

                if stream.write_all(response.as_bytes()).is_err() {
                    println!("   ⚠️ Error al enviar respuesta {}", peer_addr);
                    break;
                }
            }
            Err(e) => {
                // Caso 2: Desconexión violenta (Ctrl+C, corte de internet, etc...)
                println!("   🔴 DESCONECTADO (Forzado): {} -> {}", peer_addr, e);
                break;
            }
        }
    }
}
