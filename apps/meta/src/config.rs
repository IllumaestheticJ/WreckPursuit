//! Service configuration (environment variables for the binary; struct literal in tests).

use std::net::SocketAddr;
use std::path::PathBuf;

/// Password hashing cost.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HashCost {
    /// Argon2id library defaults (19 MiB, 2 passes). Production.
    Production,
    /// Minimal Argon2id cost. Tests and local bots only — never deploy with this.
    Fast,
}

/// Token-bucket parameters: `burst` requests at once, refilled at `per_minute`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Limit {
    /// Bucket size.
    pub burst: f64,
    /// Refill rate, requests per minute.
    pub per_minute: f64,
}

/// Rate limits per endpoint group.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RateLimits {
    /// Signups per client IP.
    pub signup_per_ip: Limit,
    /// Login attempts per client IP.
    pub login_per_ip: Limit,
    /// Login attempts per email (slows password guessing across IPs).
    pub login_per_email: Limit,
    /// Matchmaking queue/status calls per account.
    pub matchmaking_per_account: Limit,
}

impl RateLimits {
    /// Production defaults.
    pub fn production() -> Self {
        Self {
            signup_per_ip: Limit {
                burst: 5.0,
                per_minute: 2.0,
            },
            login_per_ip: Limit {
                burst: 20.0,
                per_minute: 10.0,
            },
            login_per_email: Limit {
                burst: 10.0,
                per_minute: 5.0,
            },
            matchmaking_per_account: Limit {
                burst: 30.0,
                per_minute: 120.0,
            },
        }
    }

    /// Effectively unlimited (load tests, bots).
    pub fn unlimited() -> Self {
        let l = Limit {
            burst: 1.0e9,
            per_minute: 1.0e9,
        };
        Self {
            signup_per_ip: l,
            login_per_ip: l,
            login_per_email: l,
            matchmaking_per_account: l,
        }
    }
}

/// Everything the service needs to start.
#[derive(Debug, Clone)]
pub struct Config {
    /// Listen address.
    pub listen: SocketAddr,
    /// SQLite file; `None` = in-memory (tests).
    pub db_path: Option<PathBuf>,
    /// Bearer token for `/v1/admin/*`. Empty disables the admin API.
    pub admin_token: String,
    /// Shared key instance servers present to `POST /v1/instances`.
    pub instance_key: String,
    /// Data root for economy tables (`data/`); `None` = tables embedded at build time.
    pub data_root: Option<PathBuf>,
    /// Session lifetime.
    pub session_ttl_ms: i64,
    /// Join token lifetime.
    pub join_ttl_ms: i64,
    /// Reconnect token lifetime (refreshed by heartbeats that list the player).
    pub reconnect_ttl_ms: i64,
    /// Queued tickets older than this expire.
    pub queue_ttl_ms: i64,
    /// Expected heartbeat interval; 3 missed = down.
    pub heartbeat_interval_ms: i64,
    /// Password hashing cost.
    pub hash_cost: HashCost,
    /// Rate limits.
    pub rate_limits: RateLimits,
    /// Browser origins allowed to call the player API cross-origin (`scheme://host[:port]`,
    /// or `*` for any; dev only). Empty = no CORS headers. See [`crate::cors`].
    pub cors_origins: Vec<String>,
    /// Skill matching for racers: largest clean-rating gap between a racer (or party mean) and
    /// an instance's racers at queue time. See [`crate::matchmaking`].
    pub skill_window: f64,
    /// How fast the skill window widens while a ticket waits (rating points per second), so
    /// nobody waits forever for an exact match.
    pub skill_widen_per_s: f64,
}

impl Config {
    /// Defaults suitable for a local run; secrets must still be set.
    pub fn local() -> Self {
        Self {
            listen: SocketAddr::from(([127, 0, 0, 1], 8090)),
            db_path: Some(PathBuf::from("meta.sqlite")),
            admin_token: String::new(),
            instance_key: String::new(),
            data_root: None,
            session_ttl_ms: 30 * 24 * 3_600_000,
            join_ttl_ms: 60_000,
            reconnect_ttl_ms: 10 * 60_000,
            queue_ttl_ms: 5 * 60_000,
            heartbeat_interval_ms: 5_000,
            hash_cost: HashCost::Production,
            rate_limits: RateLimits::production(),
            cors_origins: Vec::new(),
            skill_window: 200.0,
            skill_widen_per_s: 10.0,
        }
    }

    /// In-memory database, fast hashing, no rate limits, fixed secrets. Tests only.
    pub fn test() -> Self {
        Self {
            listen: SocketAddr::from(([127, 0, 0, 1], 0)),
            db_path: None,
            admin_token: "test-admin-token".into(),
            instance_key: "test-instance-key".into(),
            hash_cost: HashCost::Fast,
            rate_limits: RateLimits::unlimited(),
            ..Self::local()
        }
    }

    /// Read `WP_META_*` environment variables on top of [`Config::local`]:
    /// `WP_META_LISTEN`, `WP_META_DB`, `WP_META_ADMIN_TOKEN`, `WP_META_INSTANCE_KEY`,
    /// `WP_META_DATA`, `WP_META_CORS_ORIGINS` (comma-separated origins, or `*`),
    /// `WP_META_FAST_HASH=1` (dev only), `WP_META_NO_RATE_LIMIT=1` (dev only).
    pub fn from_env() -> Result<Self, String> {
        let mut c = Self::local();
        let var = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        if let Some(v) = var("WP_META_LISTEN") {
            c.listen = v.parse().map_err(|e| format!("WP_META_LISTEN: {e}"))?;
        }
        if let Some(v) = var("WP_META_DB") {
            c.db_path = if v == ":memory:" {
                None
            } else {
                Some(PathBuf::from(v))
            };
        }
        if let Some(v) = var("WP_META_ADMIN_TOKEN") {
            c.admin_token = v;
        }
        if let Some(v) = var("WP_META_INSTANCE_KEY") {
            c.instance_key = v;
        }
        if let Some(v) = var("WP_META_DATA") {
            c.data_root = Some(PathBuf::from(v));
        }
        if let Some(v) = var("WP_META_CORS_ORIGINS") {
            c.cors_origins = parse_origins(&v)?;
        }
        if var("WP_META_FAST_HASH").as_deref() == Some("1") {
            c.hash_cost = HashCost::Fast;
        }
        if var("WP_META_NO_RATE_LIMIT").as_deref() == Some("1") {
            c.rate_limits = RateLimits::unlimited();
        }
        if c.instance_key.len() < 16 {
            return Err("WP_META_INSTANCE_KEY must be set (>= 16 chars)".into());
        }
        if !c.admin_token.is_empty() && c.admin_token.len() < 16 {
            return Err(
                "WP_META_ADMIN_TOKEN must be >= 16 chars (or unset to disable admin)".into(),
            );
        }
        Ok(c)
    }
}

/// Parse a comma-separated origin list (`WP_META_CORS_ORIGINS`). Each entry must be `*` or
/// `http(s)://host[:port]` with no path.
pub fn parse_origins(v: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    for o in v.split(',').map(str::trim).filter(|o| !o.is_empty()) {
        let rest = o
            .strip_prefix("http://")
            .or_else(|| o.strip_prefix("https://"));
        let ok = o == "*" || rest.is_some_and(|r| !r.is_empty() && !r.contains('/'));
        if !ok {
            return Err(format!(
                "WP_META_CORS_ORIGINS: '{o}' is not '*' or 'http(s)://host[:port]'"
            ));
        }
        out.push(o.to_string());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origin_list() {
        assert_eq!(
            parse_origins(" http://localhost:5173 , https://play.example.test").unwrap(),
            ["http://localhost:5173", "https://play.example.test"]
        );
        assert_eq!(parse_origins("*").unwrap(), ["*"]);
        assert!(parse_origins("http://localhost:5173/").is_err());
        assert!(parse_origins("localhost:5173").is_err());
        assert!(parse_origins("").unwrap().is_empty());
    }
}
