// Copyright (c) 2024 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;
use thiserror::Error;
use utoipa::ToSchema;

#[derive(Error, Debug)]
pub(crate) enum ApiError {
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error("service unavailable: {0}")]
    ServiceUnavailable(String),
    #[error("internal server error")]
    InternalServerError,
    #[error("forbidden")]
    Forbidden,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status_code = match self {
            ApiError::BadRequest(_) => StatusCode::BAD_REQUEST,
            ApiError::ServiceUnavailable(_) => StatusCode::SERVICE_UNAVAILABLE,
            ApiError::InternalServerError => StatusCode::INTERNAL_SERVER_ERROR,
            ApiError::Forbidden => StatusCode::FORBIDDEN,
        };

        let body = Json(ErrorResponse {
            error_code: status_code.as_u16().to_string(),
            error_message: self.to_string(),
        });

        (status_code, body).into_response()
    }
}

/// Describes the response body of a unsuccessful HTTP request.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct ErrorResponse {
    error_code: String,
    error_message: String,
}
