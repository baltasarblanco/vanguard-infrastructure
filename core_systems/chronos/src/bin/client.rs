// Analizar el manejo estricto de buffers y el lavado (flush) de la salida estandar
use std::io::{self, Read, Write};
use std::net::TcpStream;

fn main() {
    println!("🔌 CHRONOS CLI v0.1.0 [Auditoría de Red Activa]");
    println!("   Estableciendo enlace TCP con el nodo central...");

    // 1. Fase de Conexión
    let mut stream = match TcpStream::connect("127.0.0.1:8080") {
        Ok(s) => {
            println!("   ✅ Enlace establecido exitosamente: 127.0.0.1:8080\n");
            s
        }
        Err(e) => {
            eprintln!(
                "   ❌ Fallo crítico en el socker: No se detecta un host remoto. ({})",
                e
            );
            return;
        }
    };

    let mut input = String::new();

    // 2. Bucle de Eventos (Event Loop)
    loop {
        // Generacion del Prompt Interactivo
        print!("chronos> ");
        io::stdout().flush().unwrap(); // Obliga al SO a imprimir el texto antes de pausar el hilo

        input.clear();

        // 3. Lectura de Input Local
        match io::stdin().read_line(&mut input) {
            Ok(0) => {
                // Se detectó la señal EOF (Ctrl+D)
                println!("\nTerminando sesión del CLI...");
                break;
            }
            Ok(_) => {
                let command = input.trim();

                // Validación de entrada vacía
                if command.is_empty() {
                    continue;
                }

                // Comando nativos del cliente para terminar el proceso
                if command.eq_ignore_ascii_case("exit") || command.eq_ignore_ascii_case("quit") {
                    println!("Terminando sesión del CLI...");
                    break;
                }

                // Inyectamos el salto de línea requerido por nuestro Parser
                let mut msg_to_send = command.to_string();
                msg_to_send.push('\n');

                // 4. Transmisión de Datos por Red
                if stream.write_all(msg_to_send.as_bytes()).is_err() {
                    eprintln!("❌ Anomalía detectada: Enlace TCP interrumpido por el host remoto.");
                    break;
                }

                // 5. Recepción y Decodificación de la Respuesta
                let mut buffer = [0; 512];
                match stream.read(&mut buffer) {
                    Ok(bytes_read) => {
                        if bytes_read == 0 {
                            eprintln!("❌ Anomalía detectada: El ervidor cerró el socket silenciosamente.");
                            break;
                        }
                        let response = String::from_utf8_lossy(&buffer[..bytes_read]);
                        println!("{}", response);
                    }

                    Err(e) => {
                        eprintln!("❌ Error de I/O leyendo el flujo de red: {}", e);
                        break;
                    }
                }
            }
            Err(e) => {
                eprintln!(
                    "❌ Error crítico leyendo el buffer de entrada estándar: {}",
                    e
                );
                break;
            }
        }
    }
}
