// Copyright (c) 2024 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use crate::db::ConnectionPool;

#[derive(Clone)]
pub(crate) struct StardustExtension {
    pub(crate) connection_pool: ConnectionPool,
}
