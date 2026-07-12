use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("backend '{0}' is disabled")]
    BackendDisabled(String),

    #[error("package not found: {0}")]
    PackageNotFound(String),

    #[error("configuration error: {0}")]
    Config(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("command error: {0}")]
    Command(String),

    #[error("{0}")]
    Message(String),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}
