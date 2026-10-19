//! CORS for browser clients (the testbed calling meta from another origin, e.g. Vite on
//! `http://localhost:5173`).
//!
//! Rules (kept small on purpose; no `tower-http` dependency):
//! - Off unless [`Config::cors_origins`](crate::config::Config::cors_origins) is non-empty.
//!   Entries are exact origins (`scheme://host[:port]`) or `*` (any origin; dev only).
//! - Only player-facing paths get CORS: `/v1/admin/*` and the instance API
//!   (`/v1/instances/{id}/...`) never do. Admin and instance tokens stay out of browsers; the
//!   admin dashboard at `/admin` is same-origin and needs no CORS.
//! - Auth is a bearer header, not cookies, so credentials are never allowed.
//! - Preflight (`OPTIONS` with `Access-Control-Request-Method`) from an allowed origin gets
//!   `204` with the allowed methods/headers; other requests from an allowed origin get
//!   `Access-Control-Allow-Origin` on the response. Disallowed origins pass through without
//!   CORS headers (the browser then blocks the response).

use axum::extract::{Request, State};
use axum::http::header::{
    ACCESS_CONTROL_ALLOW_HEADERS, ACCESS_CONTROL_ALLOW_METHODS, ACCESS_CONTROL_ALLOW_ORIGIN,
    ACCESS_CONTROL_MAX_AGE, ACCESS_CONTROL_REQUEST_METHOD, ORIGIN, VARY,
};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::AppState;

/// Methods a browser may use on player-facing paths.
pub const ALLOW_METHODS: &str = "GET, POST, PUT, DELETE";
/// Request headers a browser may send.
pub const ALLOW_HEADERS: &str = "authorization, content-type";
/// How long browsers may cache a preflight answer (seconds).
pub const MAX_AGE_S: &str = "600";

/// `true` when `path` may be called cross-origin (player-facing).
pub fn path_allowed(path: &str) -> bool {
    !(path.starts_with("/v1/admin") || path.starts_with("/v1/instances/"))
}

/// The `Access-Control-Allow-Origin` value for `origin`, or `None` when it is not allowed.
pub fn allow_origin(allowed: &[String], origin: &str) -> Option<String> {
    if allowed.iter().any(|a| a == "*") {
        Some("*".into())
    } else if allowed.iter().any(|a| a == origin) {
        Some(origin.to_string())
    } else {
        None
    }
}

fn origin_value(st: &AppState, path: &str, headers: &HeaderMap) -> Option<HeaderValue> {
    if st.config.cors_origins.is_empty() || !path_allowed(path) {
        return None;
    }
    let origin = headers.get(ORIGIN)?.to_str().ok()?;
    HeaderValue::from_str(&allow_origin(&st.config.cors_origins, origin)?).ok()
}

/// axum middleware (`from_fn_with_state`) applying the rules above.
pub async fn layer(State(st): State<AppState>, req: Request, next: Next) -> Response {
    let Some(allow) = origin_value(&st, req.uri().path(), req.headers()) else {
        return next.run(req).await;
    };
    let preflight = req.method() == Method::OPTIONS
        && req.headers().contains_key(ACCESS_CONTROL_REQUEST_METHOD);
    let mut resp = if preflight {
        let mut r = StatusCode::NO_CONTENT.into_response();
        let h = r.headers_mut();
        h.insert(
            ACCESS_CONTROL_ALLOW_METHODS,
            HeaderValue::from_static(ALLOW_METHODS),
        );
        h.insert(
            ACCESS_CONTROL_ALLOW_HEADERS,
            HeaderValue::from_static(ALLOW_HEADERS),
        );
        h.insert(ACCESS_CONTROL_MAX_AGE, HeaderValue::from_static(MAX_AGE_S));
        r
    } else {
        next.run(req).await
    };
    let h = resp.headers_mut();
    h.insert(ACCESS_CONTROL_ALLOW_ORIGIN, allow);
    h.append(VARY, HeaderValue::from_static("Origin"));
    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origins() {
        let list = vec!["http://localhost:5173".to_string()];
        assert_eq!(
            allow_origin(&list, "http://localhost:5173").as_deref(),
            Some("http://localhost:5173")
        );
        assert_eq!(allow_origin(&list, "http://evil.test"), None);
        assert_eq!(
            allow_origin(&["*".to_string()], "http://any.test").as_deref(),
            Some("*")
        );
        assert_eq!(allow_origin(&[], "http://localhost:5173"), None);
    }

    #[test]
    fn paths() {
        assert!(path_allowed("/v1/auth/login"));
        assert!(path_allowed("/v1/matchmaking/status"));
        assert!(path_allowed("/v1/instances"));
        assert!(!path_allowed("/v1/instances/3/heartbeat"));
        assert!(!path_allowed("/v1/admin/health"));
    }
}
