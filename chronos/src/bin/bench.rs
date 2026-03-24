use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Instant;

// Cantidad de operaciones a disparar
const NUM_REQUEST: u32 = 10_000;

fn main() {
    println!("🚀 Iniciando Benchmark de Chronos DB...");
    println!("📊 Operaciones de prueba: {}", NUM_REQUEST);

    let mut stream =
        TcpStream::connect("127.0.0.1:8080").expect("⚠️ El servidor debe estar corriendo");
    let mut buffer = [0; 512];

    // --- 📝 TEST DE ESCRITURA (SET) ---
    println!("\n⏳ Disparando 10,000 comandos SET (Escritura + Disco)...");
    let start_write = Instant::now();
    for i in 0..NUM_REQUEST {
        let msg = format!("SET bench_key_{} valor_de_prueba_numero_{}\n", i, i);
        stream.write_all(msg.as_bytes()).unwrap();
        let _ = stream.read(&mut buffer).unwrap(); // Esperar el OK del servidor
    }
    let duration_write = start_write.elapsed();
    let ops_sec_write = (NUM_REQUEST as f64 / duration_write.as_secs_f64()) as u32;

    // --- 📖 TEST DE LECTURA (GET) ---
    println!("⏳ Disparando 10,000 comandos GET (Lectura Lock-Free)...");
    let start_read = Instant::now();
    for i in 0..NUM_REQUEST {
        let msg = format!("GET bench_key_{}\n", i);
        stream.write_all(msg.as_bytes()).unwrap();
        let _ = stream.read(&mut buffer).unwrap(); // Esperar el valor del servidor
    }

    let duration_read = start_read.elapsed();
    let ops_sec_read = (NUM_REQUEST as f64 / duration_read.as_secs_f64()) as u32;

    // --- 🏆 RESULTADOS ---
    println!("\n🏆 RESULTADOS DEL BENCHMARK (Conexión TCP Secuencial):");
    println!("===========================================================");
    println!(
        "📝 ESCRITURAS (SET): {} ops/seg (Tiempo total: {:.2?})",
        ops_sec_write, duration_write
    );
    println!(
        "📖 LECTURAS (GET):   {} ops/seg (Tiempo total: {:.2?})",
        ops_sec_read, duration_read
    );
    println!("===========================================================");
}
