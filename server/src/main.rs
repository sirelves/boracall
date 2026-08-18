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
mod ratelimit;
mod signaling;
mod slug;
mod state;

use axum::{
    http::StatusCode,
    routing::{delete, get, patch, post, put},
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
    let otp = otp::OtpStore::new(pool.clone());
    let mailer = email::Mailer::new(cfg.resend_api_key.clone(), cfg.email_from.clone());

    // Limites. Os números vêm do config pra dar pra afrouxar em produção sem
    // recompilar — mas o default já é o que faz sentido.
    let limite_ip = ratelimit::RateLimiter::new(ratelimit::Politica::new(
        cfg.rl_auth_burst,
        Duration::from_secs(cfg.rl_auth_janela_secs),
    ));
    let limite_email = ratelimit::RateLimiter::new(ratelimit::Politica::new(
        cfg.rl_email_burst,
        Duration::from_secs(cfg.rl_email_janela_secs),
    ));

    // Códigos vencidos saem da tabela periodicamente. O expires_at já barra o
    // uso; isso é pra tabela não crescer sem parar.
    {
        let otp = otp.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(3600));
            loop {
                tick.tick().await;
                match otp.limpar_vencidos().await {
                    Ok(n) if n > 0 => tracing::info!(removidos = n, "otp vencidos limpos"),
                    Err(e) => tracing::warn!(error = %e, "falha limpando otp vencidos"),
                    _ => {}
                }
            }
        });
    }

    // Sem isso o mapa de baldes guarda uma entrada por IP que já passou por
    // aqui, pra sempre.
    {
        let (a, b) = (limite_ip.clone(), limite_email.clone());
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(300));
            loop {
                tick.tick().await;
                a.limpar();
                b.limpar();
            }
        });
    }

    let state = AppState {
        db: pool,
        hub,
        jwt_secret: Arc::new(cfg.jwt_secret.clone()),
        jwt_ttl_days: cfg.jwt_ttl_days,
        otp,
        mailer,
        stun_urls: cfg.stun_urls.clone(),
        turn_urls: cfg.turn_urls.clone(),
        turn_secret: cfg.turn_secret.clone().map(Arc::new),
        turn_ttl_secs: cfg.turn_ttl_secs,
        limite_ip,
        limite_email,
        max_peers_per_channel: cfg.max_peers_per_channel,
    };

    // ----------------------- routes -----------------------
    // Rotas de autenticação: limitadas por IP. É onde mora força bruta de senha,
    // criação em massa de conta e chute de código OTP.
    //
    // /auth/me fica de fora: é leitura da própria sessão, chamada no boot do app
    // e a cada troca de tela, e limitar isso só quebraria uso legítimo.
    let auth = Router::new()
        .route("/signup", post(handlers::auth::signup))
        .route("/login", post(handlers::auth::login))
        .route("/request-otp", post(handlers::auth::request_otp))
        .route("/verify-otp", post(handlers::auth::verify_otp))
        .route(
            "/request-password-reset",
            post(handlers::auth::request_password_reset),
        )
        .route("/reset-password", post(handlers::auth::reset_password))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            ratelimit::por_ip,
        ));

    let api = Router::new()
        // system
        .route("/health", get(handlers::system::health))
        .route("/version", get(handlers::system::version))
        .route("/stats", get(handlers::system::stats))
        // auth
        .nest("/auth", auth)
        .route("/auth/me", get(handlers::auth::me))
        .route("/auth/me", patch(handlers::auth::update_me))
        // servers + channels
        .route("/servers", get(handlers::servers::list_servers))
        .route("/servers", post(handlers::servers::create_server))
        .route("/servers/{slug}", get(handlers::servers::get_server))
        .route("/servers/{slug}/join", post(handlers::servers::join_server))
        .route(
            "/servers/{slug}/channels",
            post(handlers::servers::create_channel),
        )
        .route("/channels/{slug}", get(handlers::servers::get_channel))
        // WebRTC
        .route("/ice", get(handlers::ice::ice_servers))
        // mensagens
        .route(
            "/channels/{slug}/messages",
            get(handlers::messages::list_messages),
        )
        .route(
            "/channels/{slug}/messages",
            post(handlers::messages::send_message),
        )
        .route("/channels/{slug}/read", put(handlers::messages::mark_read))
        .route("/messages/{id}", patch(handlers::messages::edit_message))
        .route("/messages/{id}", delete(handlers::messages::delete_message));

    let ws = Router::new().route("/servers/{slug}", get(signaling::ws_server));

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
                .layer(TimeoutLayer::with_status_code(
                    StatusCode::REQUEST_TIMEOUT,
                    Duration::from_secs(30),
                ))
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
    let ctrl_c = async {
        tokio::signal::ctrl_c().await.ok();
    };
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
