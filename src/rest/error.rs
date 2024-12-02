use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ApiError {
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error("not found")]
    NotFound,
    #[error("service unavailable: {0}")]
    ServiceUnavailable(String),
    #[error("internal server error")]
    InternalServerError,
    #[error("forbidden")]
    Forbidden,
    #[error("bad request: {0}")]
    IotaTypes(#[from] iota_types::error::IotaError),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status_code = match self {
            ApiError::BadRequest(_) => StatusCode::BAD_REQUEST,
            ApiError::NotFound => StatusCode::NOT_FOUND,
            ApiError::ServiceUnavailable(_) => StatusCode::SERVICE_UNAVAILABLE,
            ApiError::InternalServerError => StatusCode::INTERNAL_SERVER_ERROR,
            ApiError::Forbidden => StatusCode::FORBIDDEN,
            ApiError::IotaTypes(_) => StatusCode::BAD_REQUEST,
        };

        let body = Json(ErrorBody {
            error_code: status_code.as_u16().to_string(),
            error_message: self.to_string(),
        });

        (status_code, body).into_response()
    }
}

/// Describes the response body of a unsuccessful HTTP request.
#[derive(Clone, Debug, Serialize)]
pub struct ErrorBody {
    pub error_code: String,
    pub error_message: String,
}
