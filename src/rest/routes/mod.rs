// Copyright (c) 2024 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use axum::Router;

mod v1;

pub(crate) fn router_all() -> Router {
    Router::new().merge(v1::router())
}
