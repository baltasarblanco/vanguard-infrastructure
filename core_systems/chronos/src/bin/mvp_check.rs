use std::collections::HashMap;
use std::fs::{self, OpenOptions, File};
use std::io::{Write, Seek, SeekFrom, BufWriter};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use serde::{Serialize, Deserialize};

/// --- ESTRUCTURAS ---
#[derive(Serialize, Deserialize, Debug)]
struct Entry {
    key: String,
    value: String,
    timestamp: u64,
}

#[derive(Debug)]
pub struct ChronosDB {
    path: PathBuf,
    // La Memoria: Clave -> Vector (Tiempo, Offset)
    index: HashMap<String, Vec<(u64, u64)>>,
}

impl ChronosDB {
    pub fn new(path: &str) -> Result<Self, String> {
        let path_buf = PathBuf::from(path);

        // ✅ LIMPIEZA AUTOMÁTICA: Borra la DB vieja para empezar limpio
        if path_buf.exists() {
            fs::remove_file(&path_buf).map_err(|e| e.to_string())?;
        }
        
        Ok(ChronosDB {
            path: path_buf,
            index: HashMap::new(),
        })
    }

    fn current_time() -> u64 {
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64
    }

    // 1. ESCRIBIR (SET )
    pub fn set(&mut self, key: String, value:String) -> Result<u64, String> {
        let now = ChronosDB::current_time();

        let file = OpenOptions::new()
            .create(true).append(true).open(&self.path).map_err(|e| e.to_string())?;
        let mut writer = BufWriter::new(file);

        let current_offset = writer.seek(SeekFrom::End(0)).map_err(|e| e.to_string())?;

        let entry = Entry { key: key.clone(), value: value.clone(), timestamp: now };
        bincode::serialize_into(&mut writer, &entry).map_err(|e| e.to_string())?;
        writer.flush().map_err(|e| e.to_string())?;

        // Actualizamos índice
        let history = self.index.entry(key).or_insert(Vec::new());
        history.push((now, current_offset));

        Ok(now)   
    }


    // 2. LEER INTERNO
    fn read_at(&self, offset: u64) -> Result<String, String> {
        let mut file = File::open(&self.path).map_err(|e| e.to_string())?;
        file.seek(SeekFrom::Start(offset)).map_err(|e| e.to_string())?;
        let entry: Entry = bincode::deserialize_from(&file).map_err(|e| e.to_string())?;
        Ok(entry.value)
    }

    // 3. VIAJE EN EL TIEMPO (GET AT TIME)
    pub fn get_at_time(&self, key: &str, query_time: u64) -> Option<String> {
        if let Some(history) = self.index.get(key) {
            // Buscamos el punto de corte (Partition Point)
            let index = history.partition_point(|(t, _)| *t <= query_time);
            if index == 0 {return None; } // Fue antes de que existiera el dato.
            let (_t, offset) = history[index - 1];
            return self.read_at(offset).ok();
        }
        None
    }
}

// --- SIMULACIÓN DE USUARIO ----
fn main() {
    println!("--------------------------------------------------");
    println!("🚀 INICIANDO CHRONOS MVP: SYSTEM CHECK (VERSIÓN FINAL)");
    println!("--------------------------------------------------");

    let mut db = ChronosDB::new("mvp_store.db").unwrap();

    println!("\n[1] 📝 GRABANDO HISTORIAL...");
    
    // Usamos "Estado" para diferenciar del código viejo
    let t1 = db.set("estado".to_string(), "ACTIVO".to_string()).unwrap();
    println!("   > T1 ({}) -> Estado: ACTIVO", t1);
    std::thread::sleep(std::time::Duration::from_millis(100));

    let t2 = db.set("estado".to_string(), "INACTIVO".to_string()).unwrap();
    println!("   > T2 ({}) -> Estado: INACTIVO", t2);
    std::thread::sleep(std::time::Duration::from_millis(100));

    let t3 = db.set("estado".to_string(), "BORRADO".to_string()).unwrap();
    println!("   > T3 ({}) -> Estado: BORRADO", t3);

    println!("\n✅ Datos persistidos.");

    println!("\n[2] 🕵️‍♂️ TEST DE VIAJE EN EL TIEMPO");
    let tiempo_fantasma = t1 + 50; 
    let resultado = db.get_at_time("estado", tiempo_fantasma).unwrap(); // <--- Aquí fallaba antes

    println!("   🎯 Consulta en T1 + 50ms:");
    println!("      - Esperado: 'ACTIVO'");
    println!("      - Obtenido: '{}'", resultado);

    if resultado == "ACTIVO" {
        println!("\n✨ ¡PRUEBA SUPERADA! Hito 1 Completado.");
    } else {
        println!("\n❌ FALLO: Lógica incorrecta.");
    }
}


