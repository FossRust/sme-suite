use std::sync::Arc;

use async_graphql::{Error, ErrorExtensions};
use thiserror::Error;

/// Shared GraphQL result type.
pub type ApiResult<T> = Result<T, ApiError>;

#[derive(Debug, Error, Clone)]
pub enum ApiError {
    #[error("unauthorized")]
    Unauthorized,
    #[error("resource not found")]
    NotFound,
    #[error("bad request: {0}")]
    InvalidInput(String),
    #[error("internal server error")]
    Internal(Arc<anyhow::Error>),
}

impl ApiError {
    fn code(&self) -> &'static str {
        match self {
            ApiError::Unauthorized => "UNAUTHORIZED",
            ApiError::NotFound => "NOT_FOUND",
            ApiError::InvalidInput(_) => "INVALID_INPUT",
            ApiError::Internal(_) => "INTERNAL",
        }
    }

    pub fn internal(err: anyhow::Error) -> Self {
        Self::Internal(Arc::new(err))
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(value: anyhow::Error) -> Self {
        Self::internal(value)
    }
}

impl ErrorExtensions for ApiError {
    fn extend(&self) -> Error {
        let mut err = Error::new(self.to_string());
        err = err.extend_with(|_err, e| {
            e.set("code", self.code());
        });
        if let ApiError::InvalidInput(_) = self {
            err = err.extend_with(|_err, e| {
                e.set("type", "BAD_REQUEST");
            });
        }
        err
    }
}

/// Convert any error into a GraphQL error payload while hiding internals.
pub fn internal_error(err: impl Into<anyhow::Error>) -> Error {
    ApiError::internal(err.into()).extend()
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_graphql::Value;

    #[test]
    fn internal_errors_are_masked() {
        let err = internal_error(anyhow::anyhow!("boom"));
        assert_eq!(err.message, "internal server error");
        let extra = err.extensions.as_ref().and_then(|map| map.get("code"));
        let code = extra.cloned();
        assert_eq!(code, Some(Value::from("INTERNAL")));
    }
}
