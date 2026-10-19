//! `wp-meta`: the meta service binary. Configuration via `WP_META_*` environment variables
//! (see `apps/meta/README.md`).

use std::sync::Arc;

use wp_meta::clock::SystemClock;
use wp_meta::config::{Config, HashCost};
use wp_meta::AppState;

#[tokio::main]
async fn main() {
    let config = match Config::from_env() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("wp-meta: configuration error: {e}");
            std::process::exit(2);
        }
    };
    if config.hash_cost == HashCost::Fast {
        eprintln!("wp-meta: WARNING fast password hashing enabled (development only)");
    }
    if config.admin_token.is_empty() {
        eprintln!("wp-meta: admin API disabled (WP_META_ADMIN_TOKEN unset)");
    }
    let state = match AppState::new(config, Arc::new(SystemClock)) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("wp-meta: startup failed: {e}");
            std::process::exit(1);
        }
    };
    let shutdown = async {
        let _ = tokio::signal::ctrl_c().await;
        eprintln!("wp-meta: shutting down");
    };
    if let Err(e) = wp_meta::serve(
        state,
        |addr| eprintln!("wp-meta: listening on http://{addr}"),
        shutdown,
    )
    .await
    {
        eprintln!("wp-meta: server error: {e}");
        std::process::exit(1);
    }
}
