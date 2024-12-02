use axum::Router;

mod v1;

pub(crate) fn filter_all() -> Router {
    Router::new().merge(v1::filter())
}
