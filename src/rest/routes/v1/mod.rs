// Copyright (c) 2024 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use axum::Router;
use diesel::{JoinOnDsl, prelude::*};
use serde::Deserialize;
use tracing::error;

use crate::{
    models::{ObjectType, StoredObject},
    rest::{State, error::ApiError},
    schema::{expiration_unlock_conditions::dsl::*, objects::dsl::*},
};

pub(crate) mod basic;
pub(crate) mod nft;

pub(crate) fn router() -> Router {
    Router::new().nest("/v1", basic::router().merge(nft::router()))
}

fn fetch_stored_objects(
    address: iota_types::base_types::IotaAddress,
    pagination: PaginationParams,
    state: State,
    object_type_filter: ObjectType,
) -> Result<Vec<StoredObject>, ApiError> {
    let mut conn = state.connection_pool.get_connection().map_err(|e| {
        error!("failed to get connection: {e}");
        ApiError::ServiceUnavailable(format!("failed to get connection: {}", e))
    })?;

    // Set default values for pagination if not provided
    let page = pagination.page.unwrap_or(1);
    let page_size = pagination.page_size.unwrap_or(10);

    // Calculate the offset
    let offset = (page - 1) * page_size;

    let stored_objects = objects
        .inner_join(expiration_unlock_conditions.on(id.eq(object_id)))
        .select(StoredObject::as_select())
        .filter(
            owner
                .eq(address.to_vec())
                .or(return_address.eq(address.to_vec())),
        )
        .filter(object_type.eq(object_type_filter))
        .limit(page_size as i64) // Limit the number of results
        .offset(offset as i64) // Skip the results for previous pages
        .load::<StoredObject>(&mut conn)
        .map_err(|err| {
            error!("failed to load stored objects: {}", err);
            ApiError::InternalServerError
        })?;

    Ok(stored_objects)
}

#[derive(Deserialize)]
struct PaginationParams {
    page: Option<u32>,
    page_size: Option<u32>,
}

#[cfg(test)]
fn get_free_port_for_testing_only() -> Option<u16> {
    use std::net::{SocketAddr, TcpListener};
    match TcpListener::bind("127.0.0.1:0") {
        Ok(listener) => {
            let addr: SocketAddr = listener.local_addr().ok()?;
            Some(addr.port())
        }
        Err(_) => None,
    }
}
