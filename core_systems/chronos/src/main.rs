// EL CORAZON (El punto de ENTRADA)
// Main.rs será pequeñito, limpio y elegante. El único trabajo es unir el motor y el servidor.

// Le decimos a Rust que busque los otros dos archivos

mod engine;
mod parser;
mod server; // <- AVISAMSO QUE HAY UN PARSER

use engine::{Engine, DB_PATH};
use std::process;
use std::sync::{Arc, RwLock};

fn main() {
    println!("⏳ Iniciando Chronos DB...");

    // 1. Instanciamos el Motor
    let engine = Engine::new(DB_PATH).expect("Fallo crítico al iniciar la DB");

    // 2. Lo envolvemos en nuestra barrera de hilos
    let global_db = Arc::new(RwLock::new(engine));

    // -- 🚨 PROTOCOLO DE APAGADO ELEGANTE (NUEVO) --
    // Clonamos la referencia de la DB específicamente para el vigilante
    let db_for_shutdown = Arc::clone(&global_db);

    ctrlc::set_handler(move || {
        println!("\n\n⚠️ SEÑAL DE INTERRUPCIÓN DETECTADA (Ctrl+C)");
        println!("💾 Activando protocolo de guardado de emergencia...");

        // 1. Tomamos el control absoluto (Escritura) para que nadie más modifique datos
        let mut db = db_for_shutdown.write().unwrap();

        // 2. Obligamos al motor a guardar/compactar todo en el disco de forma segura
        let _ = db.compact();

        println!("🛑 Memoria asegurada. Servidor Chronos apagado correctamente.¡Hasta la proxima, Arquitecto!");

        // 3. Salimos del programa con código 0 (Éxito)
        process::exit(0);
    }).expect("Error al inicializar el escudo SIGINT");
    // -------------------------------------------------

    // 3. Arrancamos el Servidor TCP
    server::start_server(global_db);
}
