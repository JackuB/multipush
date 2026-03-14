use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("config error: {0}")]
    Config(String),

    #[error("config validation failed:\n{}", .0.iter().map(|e| format!("  - {e}")).collect::<Vec<_>>().join("\n"))]
    ConfigValidation(Vec<String>),

    #[error("provider error: {0}")]
    Provider(String),

    #[error("rule evaluation error: {0}")]
    RuleEvaluation(String),

    #[error("recipe error: {0}")]
    Recipe(String),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Yaml(#[from] serde_yaml_ng::Error),

    #[error(transparent)]
    Json(#[from] serde_json::Error),

    #[error("{0}")]
    Other(String),
}
