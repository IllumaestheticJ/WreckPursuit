//! # wp_meta
//!
//! Meta services around the city instances (`apps/server`): accounts and sessions, progression
//! persistence (a [`wp_economy::AccountStore`] over SQLite), crews and territory, the instance
//! directory, matchmaking with parties and reconnect tokens, leaderboards with daily/weekly
//! periods, and admin/moderation with an audit log and feature flags.
//!
//! Layout: one module per domain. Domain functions take a `&Connection` (or `&mut` for
//! transactions) plus the current time and return [`error::ApiResult`]; [`routes`] is a thin
//! axum layer over them. HTTP shapes live in [`api`]; [`client::MetaClient`] is a blocking
//! client for the CLI, tests and `apps/server`. Not simulation code: wall-clock time is fine,
//! but it is always read through [`clock::Clock`] so tests control it.

#![warn(missing_docs)]

pub mod admin;
pub mod api;
pub mod auth;
pub mod client;
pub mod clock;
pub mod config;
pub mod cors;
pub mod crews;
pub mod db;
pub mod directory;
pub mod error;
pub mod instance;
pub mod leaderboards;
pub mod matchmaking;
pub mod ratelimit;
pub mod results;
pub mod routes;
pub mod store;
pub mod token;

use std::sync::Arc;

use wp_economy::Economy;

use crate::clock::Clock;
use crate::config::Config;
use crate::db::Db;
use crate::ratelimit::RateLimiter;

/// Shared state of the running service.
#[derive(Clone)]
pub struct AppState {
    /// Database.
    pub db: Db,
    /// Time source.
    pub clock: Arc<dyn Clock>,
    /// Economy tables (new accounts, admin adjustments).
    pub economy: Arc<Economy>,
    /// Configuration.
    pub config: Arc<Config>,
    /// Rate limiter.
    pub limiter: Arc<RateLimiter>,
}

impl AppState {
    /// Build the state: open + migrate the database, load the economy tables.
    pub fn new(config: Config, clock: Arc<dyn Clock>) -> Result<Self, String> {
        let db = match &config.db_path {
            Some(p) => Db::open(p),
            None => Db::open_in_memory(),
        }
        .map_err(|e| format!("database: {e}"))?;
        let economy = match &config.data_root {
            Some(root) => Economy::load(root).map_err(|e| format!("economy data: {e}"))?,
            None => Economy::embedded(),
        };
        Ok(Self {
            db,
            clock,
            economy: Arc::new(economy),
            config: Arc::new(config),
            limiter: Arc::new(RateLimiter::default()),
        })
    }

    /// Current time (Unix ms).
    pub fn now(&self) -> clock::UnixMs {
        self.clock.now_ms()
    }
}

/// Bind `state.config.listen` and serve until `shutdown` resolves. Returns the bound address
/// through `on_bound` (useful with port 0).
pub async fn serve(
    state: AppState,
    on_bound: impl FnOnce(std::net::SocketAddr),
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    let listener = tokio::net::TcpListener::bind(state.config.listen).await?;
    on_bound(listener.local_addr()?);
    let app = routes::router(state);
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown)
    .await
}
