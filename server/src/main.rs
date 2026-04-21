//! BoraCall backend — entrypoint.

#[cfg(not(target_env = "msvc"))]
#[global_allocator]
static ALLOC: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

mod auth;
mod config;
mod db;
mod email;
mod error;
mod handlers;
mod otp;
mod signaling;
mod state;

use axum::{
    routing::{get, patch, post},
    Router,
};
use std::sync::Arc;
use std::time::Duration;
use tower::ServiceBuilder;
use tower_http::{
    compression::CompressionLayer, cors::CorsLayer, timeout::TimeoutLayer, trace::TraceLayer,
};
use tracing_subscriber::EnvFilter;

use crate::{config::Config, signaling::Hub, state::AppState};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cfg = Config::from_env()?;

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::new(&cfg.log))
        .with_target(false)
        .compact()
        .init();

    tracing::info!(version = env!("CARGO_PKG_VERSION"), bind = %cfg.bind, "starting");

    let pool = db::connect(&cfg.database_url).await?;
    let hub = Arc::new(Hub::new());
    let otp = otp::OtpStore::new();
    let mailer = email::Mailer::new(cfg.resend_api_key.clone(), cfg.email_from.clone());

    let state = AppState {
        db: pool,
        hub,
        jwt_secret: Arc::new(cfg.jwt_secret.clone()),
        jwt_ttl_days: cfg.jwt_ttl_days,
        otp,
        mailer,
    };

    // ----------------------- routes -----------------------
    let api = Router::new()
        // system
        .route("/health", get(handlers::system::health))
        .route("/version", get(handlers::system::version))
        .route("/stats", get(handlers::system::stats))
        // auth
        .route("/auth/signup", post(handlers::auth::signup))
        .route("/auth/login", post(handlers::auth::login))
        .route("/auth/request-otp", post(handlers::auth::request_otp))
        .route("/auth/verify-otp", post(handlers::auth::verify_otp))
        .route("/auth/request-password-reset", post(handlers::auth::request_password_reset))
        .route("/auth/reset-password", post(handlers::auth::reset_password))
        .route("/auth/me", get(handlers::auth::me))
        .route("/auth/me", patch(handlers::auth::update_me))
        // rooms
        .route("/rooms", get(handlers::rooms::list_rooms))
        .route("/rooms", post(handlers::rooms::create_room))
        .route("/rooms/{slug}", get(handlers::rooms::get_room))
        .route("/rooms/{slug}/join", post(handlers::rooms::join_room));

    let ws = Router::new().route("/rooms/{slug}", get(signaling::ws_room));

    let cors = if cfg.cors_allow_any {
        CorsLayer::very_permissive()
    } else {
        CorsLayer::new()
    };

    let app = Router::new()
        .nest("/api", api)
        .nest("/ws", ws)
        .with_state(state)
        .layer(
            ServiceBuilder::new()
                .layer(TraceLayer::new_for_http())
                .layer(CompressionLayer::new())
                .layer(TimeoutLayer::new(Duration::from_secs(30)))
                .layer(cors),
        );

    let listener = tokio::net::TcpListener::bind(cfg.bind).await?;
    tracing::info!("listening on http://{}", cfg.bind);

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;

    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async { tokio::signal::ctrl_c().await.ok(); };
    #[cfg(unix)]
    let term = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("install sigterm")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();

    tokio::select! { _ = ctrl_c => {}, _ = term => {} };
    tracing::info!("shutting down");
}
