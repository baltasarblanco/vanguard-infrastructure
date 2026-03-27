 use logos::Logos;

#[derive(Logos, Debug, PartialEq)]
#[logos(skip r"[ \t\n\f]+")] // El Lexer ignora los espacios en blanco
pub enum CelerToken {
    // --- Palabras Clave (Keywords) ---
    #[token("RULE")]
    Rule,
    #[token("WINDOW")]
    Window,
    #[token("WHEN")]
    When,
    #[token("ACTION")]
    Action,
    #[token("LOG")]
    Log,

    // --- Símbolos ---
    #[token("{")]
    BraceOpen,
    #[token("}")]
    BraceClose,
    #[token(";")]
    Semicolon,

    // --- Valores Dinámicos ---
    #[regex(r#""[^"]*""#)] // Atrapa strings entre comillas
    StringLiteral,
    
    #[regex("[a-zA-Z_][a-zA-Z0-9_]*")] // Atrapa nombres de variables
    Identifier,
    
    #[regex(r"[0-9]+(\.[0-9]+)?")] // Atrapa números enteros y decimales
    Number,
    
    #[regex(r"<|>|==|!=|<=|>=")] // Atrapa operadores matemáticos
    Operator,
}

// ==========================================
// 🚀 TEST DE PRUEBA: El "Hola Mundo" del DSL
// ==========================================
#[cfg(test)]
mod tests {
    use super::*;
    use logos::Logos;

    #[test]
    fn probar_lexer() {
        let codigo_fuente = r#"
            RULE "High Precision Guard" {
                WHEN precision < 0.85;
                ACTION TRIGGER_FLASH;
            }
        "#;

        println!("\n🔍 Analizando regla CELER:\n{}", codigo_fuente);
        println!("--------------------------------------------------");

        let mut lexer = CelerToken::lexer(codigo_fuente);

        while let Some(token_result) = lexer.next() {
            match token_result {
                Ok(token) => println!("✅ Token: {:<15} | Texto: '{}'", format!("{:?}", token), lexer.slice()),
                Err(_) => println!("❌ ERROR: No reconozco esto: '{}'", lexer.slice()),
            }
        }
        println!("--------------------------------------------------\n");
    }
}