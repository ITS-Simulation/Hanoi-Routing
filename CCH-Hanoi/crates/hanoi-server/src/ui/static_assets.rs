use axum::http::header;
use axum::response::{Html, IntoResponse};

pub async fn handle_index() -> Html<&'static str> {
    Html(include_str!("../../static/index.html"))
}

pub async fn handle_styles() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
        include_str!("../../static/styles.css"),
    )
}

pub async fn handle_script() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
        include_str!("../../static/app.js"),
    )
}
