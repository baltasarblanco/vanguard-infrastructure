// Este es el CEREBRO (El motor LSM)
// Archivo dedicado exclusivamente a manejar el almacenamiento, los archivos y la memoria.
// Nada de INTERNET. Nada de TCP. ---->>> SOLO DATOS!!

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};

pub const DB_PATH: &str = "chronos_v3.db";

// Le decimos a Rust que esta estructura es pública
pub struct Engine {
    map: HashMap<String, String>,
    log_file: File,
}

impl Engine {
    pub fn new(filepath: &str) -> io::Result<Self> {
        let path = filepath.to_string();
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .append(true)
            .open(&path)?;

        let mut map = HashMap::new();
        println!("   📜 Rehidratando memoria desde '{}'...", path);
        let reader = BufReader::new(file.try_clone()?);

        for line in reader.lines() {
            if let Ok(record) = line {
                if let Some((k, v)) = record.split_once(',') {
                    map.insert(k.to_string(), v.to_string());
                }
            }
        }
        println!("   ✅ Memoria restaurada: {} registros.", map.len());
        Ok(Engine {
            map,
            log_file: file,
        })
    }

    pub fn set(&mut self, key: &str, value: &str) -> io::Result<()> {
        self.map.insert(key.to_string(), value.to_string());
        writeln!(self.log_file, "{},{}", key, value)?;
        Ok(())
    }

    pub fn get(&self, key: &str) -> Option<&String> {
        self.map.get(key)
    }

    pub fn compact(&mut self) -> io::Result<()> {
        let temp_path = "chronos_temp.db";
        println!("   🧹 Iniciando Compactación (Garbage Colecction)...");

        let mut temp_file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(temp_path)?;

        for (key, value) in &self.map {
            writeln!(temp_file, "{},{}", key, value)?;
        }
        temp_file.sync_all()?;
        fs::rename(temp_path, DB_PATH)?;

        self.log_file = OpenOptions::new()
            .create(true)
            .read(true)
            .append(true)
            .open(DB_PATH)?;
        println!("   ✨ Compactación terminada. Basura eliminada.");
        Ok(())
    }
}
