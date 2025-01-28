// Copyright (c) 2024 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use axum::{Router, routing::get};
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

use crate::rest::{
    ApiDoc,
    routes::{health::health, metrics::metrics},
};

pub(crate) mod health;
pub(crate) mod metrics;
pub(crate) mod v1;

pub(crate) fn router_all() -> Router {
    Router::new().merge(v1::router()).merge(
        Router::new()
            .route("/health", get(health))
            .merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", ApiDoc::openapi()))
            .merge(Router::new().route("/metrics", get(metrics))),
    )
}

#[cfg(test)]
mod test_utils {
    use diesel::{RunQueryDsl, insert_into};
    use iota_types::{balance::Balance, base_types::ObjectID, collection_types::Bag, id::UID};

    use crate::{
        db::PoolConnection,
        models::{ExpirationUnlockCondition, IotaAddress, StoredObject},
        schema::{
            expiration_unlock_conditions::dsl::expiration_unlock_conditions, objects::dsl::*,
        },
    };

    /// Get a free port for testing purposes.
    pub(crate) fn get_free_port_for_testing_only() -> Option<u16> {
        use std::net::{SocketAddr, TcpListener};
        match TcpListener::bind("127.0.0.1:0") {
            Ok(listener) => {
                let addr: SocketAddr = listener.local_addr().ok()?;
                Some(addr.port())
            }
            Err(_) => None,
        }
    }

    /// Create and insert a basic output into the database.
    pub(crate) fn create_and_insert_basic_output(
        connection: &mut PoolConnection,
        owner_address: iota_types::base_types::IotaAddress,
        balance: u64,
        unix_time: u32,
    ) -> Result<iota_types::stardust::output::basic::BasicOutput, anyhow::Error> {
        let basic_object_id = ObjectID::random();
        let basic_output = iota_types::stardust::output::basic::BasicOutput {
            id: UID::new(basic_object_id),
            balance: Balance::new(balance),
            native_tokens: Bag::default(),
            storage_deposit_return: None,
            timelock: None,
            expiration: Some(
                iota_types::stardust::output::unlock_conditions::ExpirationUnlockCondition {
                    owner: owner_address.clone(),
                    return_address: owner_address.clone(),
                    unix_time,
                },
            ),
            metadata: None,
            tag: None,
            sender: None,
        };

        let stored_object = StoredObject::new_basic_for_testing(basic_output.clone())?;

        insert_into(objects)
            .values(&stored_object)
            .execute(connection)
            .unwrap();

        let unlock_condition = ExpirationUnlockCondition {
            owner: IotaAddress(owner_address.clone()),
            return_address: IotaAddress(owner_address.clone()),
            unix_time: unix_time as i64,
            object_id: IotaAddress(basic_object_id.into()),
        };

        insert_into(expiration_unlock_conditions)
            .values(&unlock_condition)
            .execute(connection)
            .unwrap();

        Ok(basic_output)
    }

    /// Create and insert an NFT output into the database.
    pub(crate) fn create_and_insert_nft_output(
        connection: &mut PoolConnection,
        owner_address: iota_types::base_types::IotaAddress,
        balance: u64,
        unix_time: u32,
    ) -> Result<iota_types::stardust::output::nft::NftOutput, anyhow::Error> {
        let nft_object_id = ObjectID::random();
        let nft_output = iota_types::stardust::output::nft::NftOutput {
            id: UID::new(nft_object_id),
            balance: Balance::new(balance),
            native_tokens: Bag::default(),
            expiration: Some(
                iota_types::stardust::output::unlock_conditions::ExpirationUnlockCondition {
                    owner: owner_address.clone(),
                    return_address: owner_address.clone(),
                    unix_time,
                },
            ),
            storage_deposit_return: None,
            timelock: None,
        };

        let stored_object = StoredObject::new_nft_for_testing(nft_output.clone())?;

        insert_into(objects)
            .values(&stored_object)
            .execute(connection)
            .unwrap();

        let unlock_condition = ExpirationUnlockCondition {
            owner: IotaAddress(owner_address.clone()),
            return_address: IotaAddress(owner_address.clone()),
            unix_time: unix_time as i64,
            object_id: IotaAddress(nft_object_id.into()),
        };

        insert_into(expiration_unlock_conditions)
            .values(&unlock_condition)
            .execute(connection)
            .unwrap();

        Ok(nft_output)
    }
}
