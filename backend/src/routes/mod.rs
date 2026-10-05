mod health;
mod products;
mod setup;
mod users;

use axum::Router;

use crate::AppState;

pub fn router(state: AppState) -> Router {
    Router::new()
        .merge(health::router())
        .merge(setup::router())
        .merge(users::router())
        .merge(products::router())
        .with_state(state)
}
