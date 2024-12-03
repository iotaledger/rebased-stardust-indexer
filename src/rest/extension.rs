// Copyright (c) 2024 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use crate::db::ConnectionPool;

#[derive(Clone)]
pub struct StardustExtension {
    pub connection_pool: ConnectionPool,
}
