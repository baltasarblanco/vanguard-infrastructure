// EL CORAZON (El punto de ENTRADA)
// Main.rs será pequeñito, limpio y elegante. El único trabajo es unir el motor y el servidor.

// Le decimos a Rust que busque los otros dos archivos

mod engine;
mod parser;
mod server; // <- AVISAMSO QUE HAY UN PARSER

use engine::{Engine, DB_PATH};
use std::process;
use std::sync::{Arc, RwLock};
use tracing_subscriber::EnvFilter;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    tracing::info!("chronos: starting up");

    // 1. Instanciamos el Motor
    let engine = Engine::new(DB_PATH).expect("Fallo crítico al iniciar la DB");

    // 2. Lo envolvemos en nuestra barrera de hilos
    let global_db = Arc::new(RwLock::new(engine));

    // -- 🚨 PROTOCOLO DE APAGADO ELEGANTE (NUEVO) --
    // Clonamos la referencia de la DB específicamente para el vigilante
    let db_for_shutdown = Arc::clone(&global_db);

    ctrlc::set_handler(move || {
        tracing::info!("chronos: ctrl-c received, shutting down");
        tracing::info!("chronos: flushing state before exit");

        // 1. Tomamos el control absoluto (Escritura) para que nadie más modifique datos
        let mut db = db_for_shutdown.write().unwrap();

        // 2. Obligamos al motor a guardar/compactar todo en el disco de forma segura
        let _ = db.compact();

        tracing::info!("chronos: shutdown complete");

        // 3. Salimos del programa con código 0 (Éxito)
        process::exit(0);
    })
    .expect("Error al inicializar el escudo SIGINT");
    // -------------------------------------------------

    // 3. Arrancamos el Servidor TCP
    server::start_server(global_db);
}