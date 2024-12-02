use crate::db::ConnectionPool;

#[derive(Clone)]
pub struct StardustExtension {
    pub connection_pool: ConnectionPool,
}
