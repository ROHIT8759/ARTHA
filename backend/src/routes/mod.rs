pub mod backup;
mod business;
mod health;
mod inventory;
mod products;
mod purchases;
mod sales;
mod setup;
mod suppliers;
mod users;

use axum::Router;
use tower_http::cors::CorsLayer;

use crate::AppState;

pub fn router(state: AppState) -> Router {
    Router::new()
        .merge(health::router())
        .merge(backup::router())
        .merge(setup::router())
        .merge(users::router())
        .merge(products::router())
        .merge(inventory::router())
        .merge(purchases::router())
        .merge(suppliers::router())
        .merge(sales::router())
        .merge(business::router())
        .layer(CorsLayer::permissive())
        .with_state(state)
}
