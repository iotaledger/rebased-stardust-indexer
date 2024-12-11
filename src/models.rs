// Copyright (c) 2024 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

//! Rust models of the database relations.
//!
//! This module contains also conversion logic between the models
//! and the types of interest from [`iota_types`].
use derive_more::{From, Into};
use diesel::{
    deserialize::{FromSql, FromSqlRow},
    expression::AsExpression,
    prelude::*,
    serialize::{IsNull, ToSql},
    sqlite::SqliteValue,
};
use iota_json_rpc_types::{IotaData, IotaParsedData};
use iota_types::gas_coin::GAS;
use num_enum::TryFromPrimitive;

#[derive(Clone, Debug, PartialEq, Eq, Queryable, Selectable, Insertable)]
#[diesel(table_name = crate::schema::expiration_unlock_conditions)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct ExpirationUnlockCondition {
    pub owner: IotaAddress,
    pub return_address: IotaAddress,
    pub unix_time: i32,
    pub object_id: IotaAddress,
}

#[derive(Clone, Debug, PartialEq, Eq, Queryable, Selectable, Insertable)]
#[diesel(table_name = crate::schema::objects)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct StoredObject {
    pub id: IotaAddress,
    pub object_type: ObjectType,
    pub contents: Vec<u8>,
}

use iota_types::{IOTA_FRAMEWORK_ADDRESS, STARDUST_ADDRESS, balance::Balance, id::UID};
#[cfg(test)]
use iota_types::{
    base_types::SequenceNumber,
    digests::TransactionDigest,
    object::{Data, MoveObject, Object, Owner},
    stardust::output::{basic::BasicOutput, nft::NftOutput},
    supported_protocol_versions::ProtocolConfig,
};
use move_core_types::{
    annotated_value::{MoveFieldLayout, MoveStructLayout, MoveTypeLayout},
    ident_str,
    language_storage::StructTag,
};

#[cfg(test)]
impl StoredObject {
    fn new_dummy_for_testing() -> Self {
        Self {
            id: iota_types::base_types::IotaAddress::random_for_testing_only().into(),
            object_type: ObjectType::Nft,
            contents: Default::default(),
        }
    }

    pub(crate) fn new_nft_for_testing(nft: NftOutput) -> Result<Self, anyhow::Error> {
        let object = {
            let move_object = {
                MoveObject::new_from_execution(
                    iota_types::stardust::output::NftOutput::tag(GAS::type_tag()).into(),
                    SequenceNumber::default(),
                    bcs::to_bytes(&nft)?,
                    &ProtocolConfig::get_for_min_version(),
                )?
            };

            Object::new_from_genesis(
                Data::Move(move_object),
                Owner::Shared {
                    initial_shared_version: SequenceNumber::default(),
                },
                TransactionDigest::default(),
            )
        };

        StoredObject::try_from(object.clone())
    }

    pub(crate) fn new_basic_for_testing(basic: BasicOutput) -> Result<Self, anyhow::Error> {
        let object = {
            let move_object = {
                MoveObject::new_from_execution(
                    BasicOutput::tag(GAS::type_tag()).into(),
                    SequenceNumber::default(),
                    bcs::to_bytes(&basic)?,
                    &ProtocolConfig::get_for_min_version(),
                )?
            };

            Object::new_from_genesis(
                Data::Move(move_object),
                Owner::Shared {
                    initial_shared_version: SequenceNumber::default(),
                },
                TransactionDigest::default(),
            )
        };

        StoredObject::try_from(object.clone())
    }
}

impl TryFrom<iota_types::object::Object> for StoredObject {
    type Error = anyhow::Error;

    fn try_from(object: iota_types::object::Object) -> anyhow::Result<Self> {
        let object = object.into_inner();
        if !object.is_shared() {
            anyhow::bail!("not a shared migrated object");
        }
        let object_type = ObjectType::try_from(&object)?;
        let id = iota_types::base_types::IotaAddress::from(object.id()).into();
        let iota_types::object::Data::Move(move_object) = object.data else {
            anyhow::bail!("not a move object");
        };
        Ok(Self {
            id,
            object_type,
            contents: move_object.into_contents(),
        })
    }
}

impl TryFrom<StoredObject> for IotaParsedData {
    type Error = anyhow::Error;

    fn try_from(stored: StoredObject) -> Result<Self, Self::Error> {
        let layout = match stored.object_type {
            ObjectType::Basic => layout_for_basic_output(),
            ObjectType::Nft => layout_for_nft_output(),
        };

        Self::try_from_object(bcs::from_bytes(&stored.contents)?, layout)
    }
}

fn layout_for_basic_output() -> MoveStructLayout {
    let type_param = GAS::type_tag();
    MoveStructLayout {
        type_: iota_types::stardust::output::basic::BasicOutput::tag(type_param.clone()),
        fields: vec![
            MoveFieldLayout::new(
                ident_str!("id").to_owned(),
                MoveTypeLayout::Struct(UID::layout()),
            ),
            MoveFieldLayout::new(
                ident_str!("balance").to_owned(),
                MoveTypeLayout::Struct(Balance::layout(type_param)),
            ),
            MoveFieldLayout::new(
                ident_str!("native_tokens").to_owned(),
                MoveTypeLayout::Struct(MoveStructLayout {
                    type_: StructTag {
                        address: IOTA_FRAMEWORK_ADDRESS,
                        module: ident_str!("bag").to_owned(),
                        name: ident_str!("Bag").to_owned(),
                        type_params: vec![],
                    },
                    fields: vec![
                        MoveFieldLayout::new(
                            ident_str!("id").to_owned(),
                            MoveTypeLayout::Struct(UID::layout()),
                        ),
                        MoveFieldLayout::new(ident_str!("size").to_owned(), MoveTypeLayout::U64),
                    ],
                }),
            ),
            MoveFieldLayout::new(
                ident_str!("storage_deposit_return").to_owned(),
                MoveTypeLayout::Struct(MoveStructLayout {
                    type_: StructTag {
                        address: STARDUST_ADDRESS,
                        module: ident_str!("unlocks_conditions").to_owned(),
                        name: ident_str!("StorageDepositReturnUnlockCondition").to_owned(),
                        type_params: vec![],
                    },
                    fields: vec![
                        MoveFieldLayout::new(
                            ident_str!("return_address").to_owned(),
                            MoveTypeLayout::Address,
                        ),
                        MoveFieldLayout::new(
                            ident_str!("return_amount").to_owned(),
                            MoveTypeLayout::U64,
                        ),
                    ],
                }),
            ),
            MoveFieldLayout::new(
                ident_str!("timelock").to_owned(),
                MoveTypeLayout::Struct(MoveStructLayout {
                    type_: StructTag {
                        address: STARDUST_ADDRESS,
                        module: ident_str!("unlock_conditions").to_owned(),
                        name: ident_str!("TimelockUnlockCondition").to_owned(),
                        type_params: vec![],
                    },
                    fields: vec![MoveFieldLayout::new(
                        ident_str!("unix_time").to_owned(),
                        MoveTypeLayout::U32,
                    )],
                }),
            ),
            MoveFieldLayout::new(
                ident_str!("expiration").to_owned(),
                MoveTypeLayout::Struct(MoveStructLayout {
                    type_: StructTag {
                        address: STARDUST_ADDRESS,
                        module: ident_str!("unlock_conditions").to_owned(),
                        name: ident_str!("ExpirationUnlockCondition").to_owned(),
                        type_params: vec![],
                    },
                    fields: vec![
                        MoveFieldLayout::new(
                            ident_str!("owner").to_owned(),
                            MoveTypeLayout::Address,
                        ),
                        MoveFieldLayout::new(
                            ident_str!("return_address").to_owned(),
                            MoveTypeLayout::Address,
                        ),
                        MoveFieldLayout::new(
                            ident_str!("unix_time").to_owned(),
                            MoveTypeLayout::U32,
                        ),
                    ],
                }),
            ),
            MoveFieldLayout::new(
                ident_str!("metadata").to_owned(),
                MoveTypeLayout::Vector(Box::new(MoveTypeLayout::U8)),
            ),
            MoveFieldLayout::new(
                ident_str!("tag").to_owned(),
                MoveTypeLayout::Vector(Box::new(MoveTypeLayout::U8)),
            ),
            MoveFieldLayout::new(ident_str!("sender").to_owned(), MoveTypeLayout::Address),
        ],
    }
}

fn layout_for_nft_output() -> MoveStructLayout {
    unimplemented!()
}

impl TryFrom<StoredObject> for iota_types::stardust::output::basic::BasicOutput {
    type Error = anyhow::Error;

    fn try_from(stored: StoredObject) -> Result<Self, Self::Error> {
        if !matches!(stored.object_type, ObjectType::Basic) {
            anyhow::bail!("stored object is not an BasicOutput");
        }
        Ok(bcs::from_bytes(&stored.contents)?)
    }
}

impl TryFrom<StoredObject> for iota_types::stardust::output::nft::NftOutput {
    type Error = anyhow::Error;

    fn try_from(stored: StoredObject) -> Result<Self, Self::Error> {
        if !matches!(stored.object_type, ObjectType::Nft) {
            anyhow::bail!("stored object is not an NftOutput");
        }
        Ok(bcs::from_bytes(&stored.contents)?)
    }
}

#[derive(
    From, Into, PartialOrd, Ord, Debug, Copy, Clone, PartialEq, Eq, FromSqlRow, AsExpression,
)]
#[diesel(sql_type = diesel::sql_types::Binary)]
pub struct IotaAddress(pub iota_types::base_types::IotaAddress);

impl ToSql<diesel::sql_types::Binary, diesel::sqlite::Sqlite> for IotaAddress {
    fn to_sql<'b>(
        &'b self,
        out: &mut diesel::serialize::Output<'b, '_, diesel::sqlite::Sqlite>,
    ) -> diesel::serialize::Result {
        <[u8] as ToSql<diesel::sql_types::Binary, diesel::sqlite::Sqlite>>::to_sql(
            self.0.as_ref(),
            out,
        )
    }
}

impl FromSql<diesel::sql_types::Binary, diesel::sqlite::Sqlite> for IotaAddress {
    fn from_sql(bytes: SqliteValue<'_, '_, '_>) -> diesel::deserialize::Result<Self> {
        let stored = Vec::<u8>::from_sql(bytes)?;
        Ok(iota_types::base_types::IotaAddress::try_from(stored)?.into())
    }
}

#[derive(Debug, PartialEq, Eq, Copy, TryFromPrimitive, FromSqlRow, Clone, AsExpression)]
#[diesel(sql_type = diesel::sql_types::Integer)]
#[repr(u8)]
pub enum ObjectType {
    Basic,
    Nft,
}

impl TryFrom<&iota_types::object::ObjectInner> for ObjectType {
    type Error = anyhow::Error;

    fn try_from(object: &iota_types::object::ObjectInner) -> Result<Self, Self::Error> {
        let Some(struct_tag) = object.struct_tag() else {
            anyhow::bail!("source object is not a Move object");
        };
        match (struct_tag.module.as_str(), struct_tag.name.as_str()) {
            ("nft_output", "NftOutput") => Ok(Self::Nft),
            ("basic_output", "BasicOutput") => Ok(Self::Basic),
            _ => anyhow::bail!("not eligible type for indexing"),
        }
    }
}

impl ToSql<diesel::sql_types::Integer, diesel::sqlite::Sqlite> for ObjectType {
    fn to_sql<'b>(
        &'b self,
        out: &mut diesel::serialize::Output<'b, '_, diesel::sqlite::Sqlite>,
    ) -> diesel::serialize::Result {
        out.set_value(*self as isize as i32);
        Ok(IsNull::No)
    }
}

impl FromSql<diesel::sql_types::Integer, diesel::sqlite::Sqlite> for ObjectType {
    fn from_sql(bytes: SqliteValue<'_, '_, '_>) -> diesel::deserialize::Result<Self> {
        let stored = u8::try_from(i32::from_sql(bytes)?)?;
        Ok(Self::try_from(stored)?)
    }
}

#[cfg(test)]
mod tests {
    use diesel::insert_into;

    use super::*;
    use crate::{db::run_migrations, schema::objects::dsl::*};

    #[test]
    fn stored_object_round_trip() {
        let data = vec![
            StoredObject::new_dummy_for_testing(),
            StoredObject::new_dummy_for_testing(),
        ];
        let test_db = "stored_object_round_trip.db";
        let mut connection = SqliteConnection::establish(test_db).unwrap();
        run_migrations(&mut connection).unwrap();

        let rows_inserted = insert_into(objects)
            .values(&data)
            .execute(&mut connection)
            .unwrap();
        assert_eq!(rows_inserted, 2);

        let inserted = objects
            .select(StoredObject::as_select())
            .load(&mut connection)
            .unwrap();
        assert_eq!(inserted, data);
        // clean-up test db
        std::fs::remove_file(test_db).unwrap();
    }
}
