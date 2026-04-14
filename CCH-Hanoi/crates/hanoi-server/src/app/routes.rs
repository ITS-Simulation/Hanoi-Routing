use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::routing::{get, post};
use tower_http::decompression::RequestDecompressionLayer;
use tower_http::trace::TraceLayer;

use crate::api::handlers::{
    handle_customize, handle_health, handle_info, handle_query, handle_ready, handle_reset_weights,
};
use crate::api::state::AppState;

pub fn build_query_router(state: AppState) -> Router {
    let router = Router::new()
        .route("/query", post(handle_query))
        .route("/reset_weights", post(handle_reset_weights))
        .route("/info", get(handle_info))
        .route("/health", get(handle_health))
        .route("/ready", get(handle_ready));

    #[cfg(feature = "ui")]
    let router = router
        .route(
            "/evaluate_routes",
            post(crate::api::handlers::handle_evaluate_routes),
        )
        .route(
            "/traffic_overlay",
            get(crate::api::handlers::handle_traffic_overlay),
        )
        .route(
            "/camera_overlay",
            get(crate::api::handlers::handle_camera_overlay),
        );

    router.with_state(state).layer(TraceLayer::new_for_http())
}

pub fn build_customize_router(state: AppState) -> Router {
    Router::new()
        .route("/customize", post(handle_customize))
        .with_state(state)
        .layer(DefaultBodyLimit::max(64 * 1024 * 1024))
        .layer(RequestDecompressionLayer::new())
        .layer(TraceLayer::new_for_http())
}

#[cfg(feature = "ui")]
pub fn mount_ui(router: Router) -> Router {
    router
        .route("/", get(crate::ui::static_assets::handle_index))
        .route("/ui", get(crate::ui::static_assets::handle_index))
        .route(
            "/assets/cch-query.css",
            get(crate::ui::static_assets::handle_styles),
        )
        .route(
            "/assets/cch-query.js",
            get(crate::ui::static_assets::handle_script),
        )
}
