-- Your SQL goes here
CREATE TABLE IF NOT EXISTS objects (
    id BLOB NOT NULL PRIMARY KEY,
    object_type INTEGER NOT NULL,
    contents BLOB NOT NULL
);

CREATE TABLE IF NOT EXISTS expiration_unlock_conditions (
    owner BLOB NOT NULL,
    return_address BLOB NOT NULL,
    unix_time INTEGER NOT NULL,
    object_id BLOB NOT NULL PRIMARY KEY,
    FOREIGN KEY(object_id) REFERENCES objects(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS euc_owner ON expiration_unlock_conditions (
    owner
);

CREATE INDEX IF NOT EXISTS euc_return_address ON expiration_unlock_conditions (
    return_address
);
