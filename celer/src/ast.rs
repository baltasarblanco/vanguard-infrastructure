// celer/src/ast.rs

#[derive(Debug, PartialEq)]
pub enum Operator {
    LessThan,    // <
    GreaterThan, // >
    Equal,       // ==
    NotEqual,    // !=
    LessOrEq,    // <=
    GreaterOrEq, // >=
}

#[derive(Debug, PartialEq)]
pub enum ActionType {
    TriggerFlash,
    DropPacket,
    LogAnomaly(String),
}

#[derive(Debug, PartialEq)]
pub struct Condition {
    pub variable: String,  // Ej: "precision"
    pub operator: Operator, // Ej: Operator::LessThan
    pub threshold: f64,    // Ej: 0.85
}

#[derive(Debug, PartialEq)]
pub struct CelerRule {
    pub name: String,             // Ej: "High Precision Guard"
    pub condition: Condition,
    pub action: ActionType,
}