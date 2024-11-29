// @generated automatically by Diesel CLI.

diesel::table! {
    expiration_unlock_conditions (object_id) {
        owner -> Binary,
        return_address -> Binary,
        unix_time -> Integer,
        object_id -> Binary,
    }
}

diesel::table! {
    objects (id) {
        id -> Binary,
        object_type -> Integer,
        contents -> Binary,
    }
}

diesel::joinable!(expiration_unlock_conditions -> objects (object_id));

diesel::allow_tables_to_appear_in_same_query!(
    expiration_unlock_conditions,
    objects,
);
