mod health;
mod products;
mod setup;
mod users;
mod inventory;
mod purchases;
mod suppliers;

use axum::Router;

use crate::AppState;

pub fn router(state: AppState) -> Router {
    Router::new()
        .merge(health::router())
        .merge(setup::router())
        .merge(users::router())
        .merge(products::router())
        .merge(inventory::router())
        .merge(purchases::router())
        .merge(suppliers::router())
        .with_state(state)
}
