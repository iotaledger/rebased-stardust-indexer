use axum::Router;

mod basic;

pub(crate) fn filter() -> Router {
    Router::new().nest("/v1", basic::filter())
}
