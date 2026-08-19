use std::io;
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum WatchError {
    #[error("I/O error at {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: io::Error,
    },
    #[error("parse error in {path}: {message}")]
    Parse { path: String, message: String },
    #[error("compactor error for {title_id}: {message}")]
    Compactor { title_id: String, message: String },
    #[error("status source error: {0}")]
    Status(String),
}

impl WatchError {
    pub fn io(path: impl AsRef<Path>, source: io::Error) -> Self {
        Self::Io {
            path: path.as_ref().display().to_string(),
            source,
        }
    }

    pub fn parse(path: impl AsRef<Path>, message: impl Into<String>) -> Self {
        Self::Parse {
            path: path.as_ref().display().to_string(),
            message: message.into(),
        }
    }
}

pub type WatchResult<T> = Result<T, WatchError>;
