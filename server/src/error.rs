//! Errors as the API answers them: a status and an [`ErrorBody`].

use axum::Json;
use axum::extract::rejection::{BytesRejection, JsonRejection, PathRejection, QueryRejection};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use paperloom_sync_proto::ErrorBody;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ApiError {
    #[error("{0}")]
    Unauthorized(String),
    #[error("{0}")]
    Forbidden(String),
    #[error("{0}")]
    NotFound(String),
    #[error("{message}")]
    Conflict { message: String, expected_seq: Option<u64> },
    #[error("{0}")]
    BadRequest(String),
    #[error("{0}")]
    TooLarge(String),
    #[error("{0}")]
    Internal(String),
}

impl ApiError {
    pub fn conflict(message: impl Into<String>) -> Self {
        Self::Conflict { message: message.into(), expected_seq: None }
    }

    pub fn status(&self) -> StatusCode {
        match self {
            Self::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            Self::Forbidden(_) => StatusCode::FORBIDDEN,
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::Conflict { .. } => StatusCode::CONFLICT,
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::TooLarge(_) => StatusCode::PAYLOAD_TOO_LARGE,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn code(&self) -> &'static str {
        match self {
            Self::Unauthorized(_) => "unauthorized",
            Self::Forbidden(_) => "forbidden",
            Self::NotFound(_) => "not_found",
            Self::Conflict { .. } => "conflict",
            Self::BadRequest(_) => "bad_request",
            Self::TooLarge(_) => "too_large",
            Self::Internal(_) => "internal",
        }
    }

    /// A request the framework could not read: too large, or malformed.
    fn rejected(status: StatusCode, text: String) -> Self {
        if status == StatusCode::PAYLOAD_TOO_LARGE { Self::TooLarge(text) } else { Self::BadRequest(text) }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let message = match &self {
            Self::Internal(detail) => {
                tracing::error!("{detail}");
                "Something went wrong on the server.".to_string()
            }
            other => other.to_string(),
        };
        let expected_seq = match &self {
            Self::Conflict { expected_seq, .. } => *expected_seq,
            _ => None,
        };
        let body = ErrorBody { error: self.code().into(), message, expected_seq };
        (self.status(), Json(body)).into_response()
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(err: sqlx::Error) -> Self {
        Self::Internal(format!("database: {err}"))
    }
}

impl From<opendal::Error> for ApiError {
    fn from(err: opendal::Error) -> Self {
        if err.kind() == opendal::ErrorKind::NotFound {
            Self::NotFound("No such blob".into())
        } else {
            Self::Internal(format!("blob storage: {err}"))
        }
    }
}

impl From<JsonRejection> for ApiError {
    fn from(err: JsonRejection) -> Self {
        Self::rejected(err.status(), err.body_text())
    }
}

impl From<BytesRejection> for ApiError {
    fn from(err: BytesRejection) -> Self {
        Self::rejected(err.status(), err.body_text())
    }
}

impl From<PathRejection> for ApiError {
    fn from(err: PathRejection) -> Self {
        Self::BadRequest(err.body_text())
    }
}

impl From<QueryRejection> for ApiError {
    fn from(err: QueryRejection) -> Self {
        Self::BadRequest(err.body_text())
    }
}
