// Copyright (c) 2024 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use axum::{
    async_trait,
    extract::{FromRequestParts, Path as AxumPath},
    http::request::Parts,
};
use serde::de::DeserializeOwned;

use crate::rest::error::ApiError;

// We define our own `Path` extractor that customizes the error from
// `axum::extract::Path`
pub(crate) struct Path<T>(pub T);

#[async_trait]
impl<S, T> FromRequestParts<S> for Path<T>
where
    S: Send + Sync,
    T: DeserializeOwned + Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        match AxumPath::<T>::from_request_parts(parts, state).await {
            Ok(value) => Ok(Self(value.0)),
            Err(e) => Err(ApiError::BadRequest(e.to_string())),
        }
    }
}
