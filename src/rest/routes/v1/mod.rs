// Copyright (c) 2024 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use axum::Router;

mod basic;
mod nft;

pub(crate) fn router() -> Router {
    Router::new().nest("/v1", basic::router().merge(nft::router()))
}
