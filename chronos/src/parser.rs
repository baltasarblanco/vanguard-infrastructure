// Definimos todos los comandos válidos en Chronos
pub enum Command {
    Set(String, String), // SET requiere una llave y un valor
    Get(String),         // GET requiere solo una llave
    Del(String),
    Compact,
    Ping,
    Unknown,
}

// Esta función toma el texto sucio de la red y lo convierte en un 'Command'
pub fn parse(input: &str) -> Command {
    let parts: Vec<&str> = input.trim().split_whitespace().collect();

    match parts[0].to_uppercase().as_str() {
        "SET" if parts.len() >= 3 => Command::Set(parts[1].to_string(), parts[2..].join(" ")),
        "GET" if parts.len() == 2 => Command::Get(parts[1].to_string()),
        "DEL" if parts.len() == 2 => Command::Del(parts[1].to_string()), // <- NUEVO RECONOCIMIENTO
        "PING" => Command::Ping,
        "COMPACT" => Command::Compact,
        _ => Command::Unknown,
    }
}
