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

#[derive(Queryable, Selectable, Insertable)]
#[diesel(table_name = crate::schema::expiration_unlock_conditions)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct ExpirationUnlockCondition {
    pub owner: IotaAddress,
    pub return_address: IotaAddress,
    pub unix_time: i32,
    pub object_id: IotaAddress,
}

#[derive(Queryable, Selectable, Insertable)]
#[diesel(table_name = crate::schema::objects)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct StoredObject {
    pub id: IotaAddress,
    pub object_type: ObjectType,
    pub contents: Vec<u8>,
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

#[derive(From, Into, Debug, PartialEq, Eq, FromSqlRow, AsExpression)]
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
            ("nft", "NftOutput") => Ok(Self::Nft),
            ("basic", "BasicOutput") => Ok(Self::Basic),
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
