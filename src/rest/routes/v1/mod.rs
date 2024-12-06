// Copyright (c) 2024 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use axum::Router;
use diesel::{JoinOnDsl, prelude::*};
use serde::Deserialize;
use tracing::error;

use crate::{
    db::PoolConnection,
    models::{ObjectType, StoredObject},
    rest::error::ApiError,
    schema::{expiration_unlock_conditions::dsl::*, objects::dsl::*},
};

mod basic;
mod nft;

pub(crate) fn router() -> Router {
    Router::new().nest("/v1", basic::router().merge(nft::router()))
}

fn fetch_stored_objects(
    conn: &mut PoolConnection,
    extracted_address_filter: &[u8],
    object_type_filter: ObjectType,
    page_size: usize,
    offset: usize,
) -> Result<Vec<StoredObject>, ApiError> {
    let stored_objects = objects
        .inner_join(expiration_unlock_conditions.on(id.eq(object_id)))
        .select(StoredObject::as_select())
        .filter(
            owner
                .eq(extracted_address_filter.to_vec())
                .or(return_address.eq(extracted_address_filter.to_vec())),
        )
        .filter(object_type.eq(object_type_filter))
        .limit(page_size as i64) // Limit the number of results
        .offset(offset as i64) // Skip the results for previous pages
        .load::<StoredObject>(conn)
        .map_err(|err| {
            error!("Failed to load stored objects: {}", err);
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
