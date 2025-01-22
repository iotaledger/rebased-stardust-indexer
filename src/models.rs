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
use num_enum::TryFromPrimitive;

#[derive(Clone, Debug, PartialEq, Eq, Queryable, Selectable, Insertable, AsChangeset)]
#[diesel(table_name = crate::schema::expiration_unlock_conditions)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct ExpirationUnlockCondition {
    pub owner: IotaAddress,
    pub return_address: IotaAddress,
    #[diesel(sql_type = diesel::sql_types::BigInt)]
    pub unix_time: i64,
    pub object_id: IotaAddress,
}

impl TryFrom<iota_types::stardust::output::basic::BasicOutput> for ExpirationUnlockCondition {
    type Error = anyhow::Error;

    fn try_from(
        basic: iota_types::stardust::output::basic::BasicOutput,
    ) -> Result<Self, Self::Error> {
        let Some(expiration) = basic.expiration else {
            anyhow::bail!("expiration unlock condition does not exists");
        };

        Ok(Self {
            owner: IotaAddress(expiration.owner),
            return_address: IotaAddress(expiration.return_address),
            unix_time: expiration.unix_time as i64,
            object_id: IotaAddress(iota_types::base_types::IotaAddress::from(
                *basic.id.object_id(),
            )),
        })
    }
}

impl TryFrom<iota_types::stardust::output::nft::NftOutput> for ExpirationUnlockCondition {
    type Error = anyhow::Error;

    fn try_from(nft: iota_types::stardust::output::nft::NftOutput) -> Result<Self, Self::Error> {
        let Some(expiration) = nft.expiration else {
            anyhow::bail!("expiration unlock condition does not exists");
        };

        Ok(Self {
            owner: IotaAddress(expiration.owner),
            return_address: IotaAddress(expiration.return_address),
            unix_time: expiration.unix_time as i64,
            object_id: IotaAddress(iota_types::base_types::IotaAddress::from(
                *nft.id.object_id(),
            )),
        })
    }
}

impl TryFrom<StoredObject> for ExpirationUnlockCondition {
    type Error = anyhow::Error;

    fn try_from(stored_object: StoredObject) -> Result<Self, Self::Error> {
        match stored_object.object_type {
            ObjectType::Basic => Self::try_from(
                iota_types::stardust::output::basic::BasicOutput::try_from(stored_object)?,
            ),
            ObjectType::Nft => Self::try_from(
                iota_types::stardust::output::nft::NftOutput::try_from(stored_object)?,
            ),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Queryable, Selectable, Insertable, AsChangeset)]
#[diesel(table_name = crate::schema::objects)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct StoredObject {
    pub id: IotaAddress,
    pub object_type: ObjectType,
    pub contents: Vec<u8>,
}

#[cfg(test)]
use iota_types::{
    base_types::SequenceNumber,
    digests::TransactionDigest,
    gas_coin::GAS,
    object::{Data, MoveObject, Object, Owner},
    stardust::output::{basic::BasicOutput, nft::NftOutput},
    supported_protocol_versions::ProtocolConfig,
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

#[derive(Clone, Debug, PartialEq, Eq, Queryable, Selectable, Insertable, AsChangeset)]
#[diesel(table_name = crate::schema::last_checkpoint_sync)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct LastCheckpointSync {
    #[diesel(sql_type = diesel::sql_types::BigInt)]
    pub sequence_number: i64,
    pub task_id: String,
}

#[cfg(test)]
mod tests {
    use diesel::insert_into;

    use super::*;
    use crate::{
        db::{OBJECTS_MIGRATIONS, run_migrations},
        schema::objects::dsl::*,
    };

    #[test]
    fn stored_object_round_trip() {
        let data = vec![
            StoredObject::new_dummy_for_testing(),
            StoredObject::new_dummy_for_testing(),
        ];
        let test_db = "stored_object_round_trip.db";
        let mut connection = SqliteConnection::establish(test_db).unwrap();
        run_migrations(&mut connection, OBJECTS_MIGRATIONS).unwrap();

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
