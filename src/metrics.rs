// Copyright (c) 2024 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use prometheus::{
    IntCounter, IntGauge, Registry, register_int_counter_with_registry,
    register_int_gauge_with_registry,
};

/// Metrics for the service.
#[derive(Clone)]
pub struct Metrics {
    pub last_checkpoint_checked: IntGauge,
    pub last_checkpoint_indexed: IntGauge,
    pub indexed_basic_outputs_count: IntCounter,
    pub indexed_nft_outputs_count: IntCounter,
}

impl Metrics {
    pub fn new(registry: &Registry) -> Self {
        Self {
            last_checkpoint_checked: register_int_gauge_with_registry!(
                "last_checkpoint_checked",
                "The last checkpoint received from the remote store",
                registry,
            )
            .unwrap(),
            last_checkpoint_indexed: register_int_gauge_with_registry!(
                "last_checkpoint_indexed",
                "The last checkpoint that was indexed",
                registry,
            )
            .unwrap(),
            indexed_basic_outputs_count: register_int_counter_with_registry!(
                "indexed_basic_outputs_count",
                "The total number of basic outputs indexed",
                registry,
            )
            .unwrap(),
            indexed_nft_outputs_count: register_int_counter_with_registry!(
                "indexed_nft_outputs_count",
                "The total number of NFT outputs indexed",
                registry,
            )
            .unwrap(),
        }
    }
}
