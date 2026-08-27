use thiserror::Error;

#[derive(Error, Debug)]
pub enum KonkonError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("no engine matched game directory {0}")]
    NoEngineMatch(String),
    #[error("malformed {container}: {detail}")]
    Format {
        container: &'static str,
        detail: String,
    },
    #[error("round-trip mismatch in {file}: {detail}")]
    RoundTrip { file: String, detail: String },
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, KonkonError>;
