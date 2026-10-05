mod health;
mod products;
mod setup;
mod users;
mod inventory;

use axum::Router;

use crate::AppState;

pub fn router(state: AppState) -> Router {
    Router::new()
        .merge(health::router())
        .merge(setup::router())
        .merge(users::router())
        .merge(products::router())
        .merge(inventory::router())
        .with_state(state)
}
