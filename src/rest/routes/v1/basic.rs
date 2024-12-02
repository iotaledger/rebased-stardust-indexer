use axum::{
    Extension, Json, Router,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use iota_types::base_types::ObjectID;
use serde::Serialize;

use crate::{
    impl_into_response,
    rest::{error::ApiError, extractors::custom_path::ExtractPath},
};
use crate::rest::extension::StardustExtension;

pub(crate) fn filter() -> Router {
    Router::new().route("/basic/:basic_id", get(basic))
}

async fn basic(
    ExtractPath(object_id): ExtractPath<ObjectID>
    Extension(state): Extension<StardustExtension>,
) -> Result<BasicResponse, ApiError> {
    let conn = state.connection_pool.get_connection().map_err(|_| ApiError::ServiceUnavailable("Failed to get connection".to_string()))?;

    conn.insert

    Ok(BasicResponse {
        basic_id: object_id,
    })
}

#[derive(Clone, Copy, Debug, Serialize)]
struct BasicResponse {
    basic_id: ObjectID,
}

impl_into_response!(BasicResponse);
