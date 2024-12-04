// Copyright (c) 2024 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use std::net::{SocketAddr, TcpListener};

use axum::Router;
use serde::Deserialize;

mod basic;
mod nft;

pub(crate) fn router() -> Router {
    Router::new().nest("/v1", basic::router().merge(nft::router()))
}

#[derive(Deserialize)]
struct PaginationParams {
    page: Option<u32>,
    page_size: Option<u32>,
}

#[cfg(test)]
fn get_free_port() -> Option<u16> {
    match TcpListener::bind("127.0.0.1:0") {
        Ok(listener) => {
            let addr: SocketAddr = listener.local_addr().ok()?;
            Some(addr.port())
        }
        Err(_) => None,
    }
}
