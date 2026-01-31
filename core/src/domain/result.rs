//! Result and error types for the core library

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// COMMENT: these error types are new compared to
/// the old Python CLI code. Explain why you added these,
/// I think they are unnecessary but I'm open to it.
/// Core library error type
#[derive(Error, Debug)]
pub enum Error {
    #[error("Database error: {0}")]
    Database(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Encryption error: {0}")]
    Encryption(String),

    #[error("Sync error: {0}")]
    Sync(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("{0}")]
    Other(String),
}

impl Error {
    /// Create a database error
    pub fn database(msg: impl Into<String>) -> Self {
        Self::Database(msg.into())
    }

    /// Create a not found error
    pub fn not_found(msg: impl Into<String>) -> Self {
        Self::NotFound(msg.into())
    }

    /// Create a validation error
    pub fn validation(msg: impl Into<String>) -> Self {
        Self::Validation(msg.into())
    }
}

/// Core library result type
pub type Result<T> = std::result::Result<T, Error>;

/// Operation result with optional context (for FFI serialization)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationResult<T> {
    pub success: bool,
    pub data: Option<T>,
    pub error: Option<String>,
    pub context: Option<HashMap<String, serde_json::Value>>,
}

impl<T> OperationResult<T> {
    /// Create a successful result
    pub fn ok(data: T) -> Self {
        Self {
            success: true,
            data: Some(data),
            error: None,
            context: None,
        }
    }

    /// Create a successful result with context
    pub fn ok_with_context(data: T, context: HashMap<String, serde_json::Value>) -> Self {
        Self {
            success: true,
            data: Some(data),
            error: None,
            context: Some(context),
        }
    }

    /// Create a failed result
    pub fn fail(error: impl Into<String>) -> Self {
        Self {
            success: false,
            data: None,
            error: Some(error.into()),
            context: None,
        }
    }

    /// Create a failed result with context
    pub fn fail_with_context(
        error: impl Into<String>,
        context: HashMap<String, serde_json::Value>,
    ) -> Self {
        Self {
            success: false,
            data: None,
            error: Some(error.into()),
            context: Some(context),
        }
    }
}

impl<T> From<Result<T>> for OperationResult<T> {
    fn from(result: Result<T>) -> Self {
        match result {
            Ok(data) => Self::ok(data),
            Err(e) => Self::fail(e.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_operation_result_ok() {
        let result: OperationResult<i32> = OperationResult::ok(42);
        assert!(result.success);
        assert_eq!(result.data, Some(42));
        assert!(result.error.is_none());
    }

    #[test]
    fn test_operation_result_fail() {
        let result: OperationResult<i32> = OperationResult::fail("Something went wrong");
        assert!(!result.success);
        assert!(result.data.is_none());
        assert_eq!(result.error, Some("Something went wrong".to_string()));
    }

    #[test]
    fn test_from_result() {
        let ok: Result<i32> = Ok(42);
        let result: OperationResult<i32> = ok.into();
        assert!(result.success);

        let err: Result<i32> = Err(Error::validation("bad input"));
        let result: OperationResult<i32> = err.into();
        assert!(!result.success);
        assert!(result.error.unwrap().contains("Validation error"));
    }
}
