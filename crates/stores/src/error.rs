use std::io;
use std::path::Path;

use crate::model::StoreId;

/// Recoverable per-store warning. Discovery never fails the whole run
/// because a launcher is missing or one manifest is corrupt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreWarning {
    pub store: StoreId,
    pub message: String,
}

impl StoreWarning {
    pub fn new(store: StoreId, message: impl Into<String>) -> Self {
        Self {
            store,
            message: message.into(),
        }
    }
}

/// Structured errors for index I/O and parse failures.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("I/O error at {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: io::Error,
    },
    #[error("parse error in {path}: {message}")]
    Parse { path: String, message: String },
    #[error("registry error at {key}: {message}")]
    Registry { key: String, message: String },
    #[error("refused to touch forbidden path {path}: {reason}")]
    Forbidden { path: String, reason: String },
}

impl StoreError {
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

    pub fn registry(key: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Registry {
            key: key.into(),
            message: message.into(),
        }
    }

    pub fn forbidden(path: impl AsRef<Path>, reason: impl Into<String>) -> Self {
        Self::Forbidden {
            path: path.as_ref().display().to_string(),
            reason: reason.into(),
        }
    }

    pub fn is_not_found(&self) -> bool {
        match self {
            Self::Io { source, .. } => source.kind() == io::ErrorKind::NotFound,
            Self::Registry { message, .. } => message.contains("not found"),
            _ => false,
        }
    }
}

pub type StoreResult<T> = Result<T, StoreError>;
