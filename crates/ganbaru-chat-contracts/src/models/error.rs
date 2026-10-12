use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatErrorCode {
    Validation,
    NotFound,
    Conflict,
    StaleRevision,
    InvalidStateTransition,
    Busy,
    CapabilityUnsupported,
    DriverUnavailable,
    ExecutableMissing,
    UnsupportedVersion,
    AuthenticationRequired,
    ConfigurationInvalid,
    TransportUnavailable,
    ResumeNotFound,
    Protocol,
    Permission,
    Timeout,
    Cancelled,
    Persistence,
    Internal,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatError {
    pub code: ChatErrorCode,
    pub message: String,
    pub field: Option<String>,
    pub recoverable: bool,
    pub details: Option<Box<Value>>,
}

pub type ChatResult<T> = Result<T, ChatError>;

impl ChatError {
    pub fn new(code: ChatErrorCode, message: impl Into<String>, recoverable: bool) -> Self {
        Self {
            code,
            message: message.into(),
            field: None,
            recoverable,
            details: None,
        }
    }

    pub fn validation(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: ChatErrorCode::Validation,
            message: message.into(),
            field: Some(field.into()),
            recoverable: true,
            details: None,
        }
    }

    pub fn unsupported(message: impl Into<String>) -> Self {
        Self::new(ChatErrorCode::CapabilityUnsupported, message, true)
    }

    pub fn driver_unavailable(message: impl Into<String>) -> Self {
        Self::new(ChatErrorCode::DriverUnavailable, message, true)
    }

    pub fn invalid_transition(message: impl Into<String>) -> Self {
        Self::new(ChatErrorCode::InvalidStateTransition, message, false)
    }
}

impl fmt::Display for ChatError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.field {
            Some(field) => write!(formatter, "{field}: {}", self.message),
            None => formatter.write_str(&self.message),
        }
    }
}

impl std::error::Error for ChatError {}
