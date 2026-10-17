//! The error shape shared by Music library commands and context assignments.

use serde::Serialize;
use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MusicLibraryErrorCode {
    Validation,
    NotFound,
    Conflict,
    StaleWrite,
    Database,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicLibraryError {
    pub code: MusicLibraryErrorCode,
    pub message: String,
    pub field: Option<String>,
}

pub type MusicLibraryResult<T> = Result<T, MusicLibraryError>;

impl MusicLibraryError {
    pub fn validation(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: MusicLibraryErrorCode::Validation,
            message: message.into(),
            field: Some(field.into()),
        }
    }

    pub fn database(context: &str, error: sqlx::Error) -> Self {
        Self {
            code: MusicLibraryErrorCode::Database,
            message: format!("{context}: {error}"),
            field: None,
        }
    }

    pub fn not_found(entity: &str, id: &str) -> Self {
        Self {
            code: MusicLibraryErrorCode::NotFound,
            message: format!("{entity} '{id}' was not found"),
            field: None,
        }
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self {
            code: MusicLibraryErrorCode::Conflict,
            message: message.into(),
            field: None,
        }
    }

    pub fn stale(entity: &str, id: &str) -> Self {
        Self {
            code: MusicLibraryErrorCode::StaleWrite,
            message: format!("{entity} '{id}' changed before this update was saved"),
            field: None,
        }
    }

    pub fn runtime(context: &str, message: impl fmt::Display) -> Self {
        Self {
            code: MusicLibraryErrorCode::Database,
            message: format!("{context}: {message}"),
            field: None,
        }
    }
}

impl fmt::Display for MusicLibraryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.field {
            Some(field) => write!(formatter, "{}: {}", field, self.message),
            None => formatter.write_str(&self.message),
        }
    }
}

impl std::error::Error for MusicLibraryError {}
