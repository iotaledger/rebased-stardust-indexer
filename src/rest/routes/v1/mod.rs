// Copyright (c) 2024 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use std::sync::atomic::Ordering;

use axum::Router;
use diesel::{JoinOnDsl, prelude::*};
use serde::Deserialize;
use tracing::error;

use crate::{
    models::{ObjectType, StoredObject},
    rest::{State, error::ApiError},
    schema::{expiration_unlock_conditions::dsl::*, objects::dsl::*},
    sync::LATEST_CHECKPOINT_UNIX_TIMESTAMP_MS,
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

    // Latest checkpoint unix timestamp in seconds
    let checkpoint_unix_timestamp_s = LATEST_CHECKPOINT_UNIX_TIMESTAMP_MS
        .get()
        .ok_or(ApiError::ServiceUnavailable(
            "latest checkpoint not synced yet".to_string(),
        ))?
        .load(Ordering::SeqCst) as i64
        / 1000; // Convert to seconds for comparison

    let stored_objects = objects
        .inner_join(expiration_unlock_conditions.on(id.eq(object_id)))
        .select(StoredObject::as_select())
        .filter(object_type.eq(object_type_filter))
        .filter(
            owner
                .eq(address.to_vec())
                .and(unix_time.gt(checkpoint_unix_timestamp_s)) // Owner condition before expiration
                .or(
                    return_address
                        .eq(address.to_vec())
                        .and(unix_time.le(checkpoint_unix_timestamp_ms)), /* Return condition
                                                                           * after
                                                                           * expiration */
                ),
        )
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

pub(crate) mod responses {
    use serde::{Deserialize, Serialize};
    use utoipa::ToSchema;

    use crate::impl_into_response;

    #[derive(Clone, Debug, Serialize, ToSchema)]
    pub(crate) struct BasicOutputVec(pub(crate) Vec<BasicOutput>);
    impl_into_response!(BasicOutputVec);

    #[derive(Clone, Debug, Serialize, ToSchema)]
    pub(crate) struct NftOutputVec(pub(crate) Vec<NftOutput>);
    impl_into_response!(NftOutputVec);

    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq, ToSchema)]
    pub(crate) struct BasicOutput {
        pub(crate) id: String,
        pub(crate) balance: Balance,
        pub(crate) native_tokens: Bag,
        pub(crate) storage_deposit_return: Option<StorageDepositReturn>,
        pub(crate) timelock: Option<Timelock>,
        pub(crate) expiration: Option<Expiration>,
        pub(crate) metadata: Option<Vec<u8>>,
        pub(crate) tag: Option<Vec<u8>>,
        pub(crate) sender: Option<String>,
    }

    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq, ToSchema)]
    pub(crate) struct NftOutput {
        pub(crate) id: String,
        pub(crate) balance: Balance,
        pub(crate) native_tokens: Bag,
        pub(crate) storage_deposit_return: Option<StorageDepositReturn>,
        pub(crate) timelock: Option<Timelock>,
        pub(crate) expiration: Option<Expiration>,
    }

    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq, ToSchema)]
    pub(crate) struct Balance {
        pub(crate) value: u64,
    }

    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq, ToSchema)]
    pub(crate) struct Bag {
        pub(crate) id: String,
        pub(crate) size: u64,
    }

    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq, ToSchema)]
    pub(crate) struct StorageDepositReturn {
        pub(crate) return_address: String,
        pub(crate) return_amount: u64,
    }

    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq, ToSchema)]
    pub(crate) struct Timelock {
        pub(crate) unix_time: u64,
    }

    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq, ToSchema)]
    pub(crate) struct Expiration {
        pub(crate) owner: String,
        pub(crate) return_address: String,
        pub(crate) unix_time: u64,
    }

    impl From<iota_types::stardust::output::basic::BasicOutput> for BasicOutput {
        fn from(output: iota_types::stardust::output::basic::BasicOutput) -> Self {
            Self {
                id: output.id.object_id().to_string(),
                balance: Balance {
                    value: output.balance.value(),
                },
                native_tokens: Bag {
                    id: output.native_tokens.id.object_id().to_string(),
                    size: output.native_tokens.size,
                },
                storage_deposit_return: output.storage_deposit_return.map(|x| {
                    StorageDepositReturn {
                        return_address: x.return_address.to_string(),
                        return_amount: x.return_amount,
                    }
                }),
                timelock: output.timelock.map(|x| Timelock {
                    unix_time: x.unix_time as u64,
                }),
                expiration: output.expiration.map(|x| Expiration {
                    owner: x.owner.to_string(),
                    return_address: x.return_address.to_string(),
                    unix_time: x.unix_time as u64,
                }),
                metadata: output.metadata,
                tag: output.tag,
                sender: output.sender.map(|x| x.to_string()),
            }
        }
    }

    impl From<iota_types::stardust::output::nft::NftOutput> for NftOutput {
        fn from(output: iota_types::stardust::output::nft::NftOutput) -> Self {
            Self {
                id: output.id.object_id().to_string(),
                balance: Balance {
                    value: output.balance.value(),
                },
                native_tokens: Bag {
                    id: output.native_tokens.id.object_id().to_string(),
                    size: output.native_tokens.size,
                },
                storage_deposit_return: output.storage_deposit_return.map(|x| {
                    StorageDepositReturn {
                        return_address: x.return_address.to_string(),
                        return_amount: x.return_amount,
                    }
                }),
                timelock: output.timelock.map(|x| Timelock {
                    unix_time: x.unix_time as u64,
                }),
                expiration: output.expiration.map(|x| Expiration {
                    owner: x.owner.to_string(),
                    return_address: x.return_address.to_string(),
                    unix_time: x.unix_time as u64,
                }),
            }
        }
    }
}
