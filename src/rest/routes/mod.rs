// Copyright (c) 2024 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use axum::Router;

mod v1;

pub(crate) fn filter_all() -> Router {
    Router::new().merge(v1::filter())
}
