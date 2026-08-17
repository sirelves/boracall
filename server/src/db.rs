//! Postgres pool + migrations. Tuned for a signaling workload: many short reads
//! (auth, room lookup), periodic writes (memberships, call events), bursty under
//! room join/leave storms.

use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::{ConnectOptions, PgPool};
use std::str::FromStr;
use std::time::Duration;

pub async fn connect(url: &str) -> anyhow::Result<PgPool> {
    let mut opts = PgConnectOptions::from_str(url)?.application_name("boracall-server");

    // Keep server logs clean — per-statement noise at debug level.
    opts = opts.log_statements(tracing::log::LevelFilter::Debug);

    // Pool sizing rule of thumb: 4× CPU for a mostly I/O-bound service, clamped.
    let max = (num_cpus::get() as u32 * 4).clamp(8, 64);

    let pool = PgPoolOptions::new()
        .max_connections(max)
        .min_connections(2)
        .acquire_timeout(Duration::from_secs(5))
        .idle_timeout(Some(Duration::from_secs(300)))
        .test_before_acquire(true)
        .connect_with(opts)
        .await?;

    sqlx::migrate!("./migrations").run(&pool).await?;

    tracing::info!(max_connections = max, "database ready (postgres)");
    Ok(pool)
}
