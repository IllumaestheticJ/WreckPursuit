//! axum router: a thin layer that authenticates, rate-limits, locks the database and calls the
//! domain modules. Paths and shapes are documented in `apps/meta/openapi.yaml`.

use std::collections::BTreeMap;
use std::net::SocketAddr;

use axum::extract::{ConnectInfo, FromRequest, FromRequestParts, Path, Query, Request, State};
use axum::http::header::{AUTHORIZATION, CONTENT_TYPE};
use axum::http::request::Parts;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use wp_contracts::PlayerId;

use crate::api::*;
use crate::error::{ApiError, ApiResult};
use crate::leaderboards::Period;
use crate::{admin, auth, crews, directory, leaderboards, matchmaking, results, store, AppState};

/// The OpenAPI document, served at `/openapi.yaml`.
pub const OPENAPI_YAML: &str = include_str!("../openapi.yaml");
const DASHBOARD_HTML: &str = include_str!("dashboard.html");

/// Build the router.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/openapi.yaml", get(openapi))
        .route("/admin", get(dashboard))
        // auth
        .route("/v1/auth/providers", get(providers))
        .route("/v1/auth/signup", post(signup))
        .route("/v1/auth/login", post(login))
        .route("/v1/auth/logout", post(logout))
        .route("/v1/auth/link", post(link))
        // player
        .route("/v1/me", get(me))
        .route("/v1/me/economy", get(my_economy))
        .route("/v1/me/events", get(my_events))
        .route("/v1/flags", get(public_flags))
        .route("/v1/leaderboards/{board}", get(leaderboard))
        // parties & crews
        .route("/v1/parties", post(create_party))
        .route("/v1/parties/leave", post(leave_party))
        .route("/v1/parties/{id}", get(get_party))
        .route("/v1/parties/{id}/join", post(join_party))
        .route("/v1/crews", post(create_crew))
        .route("/v1/crews/leave", post(leave_crew))
        .route("/v1/crews/{id}", get(get_crew))
        .route("/v1/crews/{id}/join", post(join_crew))
        // matchmaking
        .route("/v1/matchmaking/queue", post(mm_queue).delete(mm_cancel))
        .route("/v1/matchmaking/status", get(mm_status))
        .route("/v1/matchmaking/reconnect", post(mm_reconnect))
        // instances
        .route("/v1/instances", get(list_instances).post(register_instance))
        .route("/v1/instances/{id}", delete(deregister_instance))
        .route("/v1/instances/{id}/heartbeat", post(heartbeat))
        .route("/v1/instances/{id}/join/verify", post(verify_join))
        .route("/v1/instances/{id}/results/race", post(report_race))
        .route("/v1/instances/{id}/results/pursuit", post(report_pursuit))
        .route("/v1/instances/{id}/territory", post(report_territory))
        .route(
            "/v1/instances/{id}/accounts/{account}/economy",
            get(instance_load_economy),
        )
        .route(
            "/v1/instances/{id}/accounts/{account}/economy/commit",
            post(instance_commit_economy),
        )
        .route(
            "/v1/instances/{id}/accounts/{account}/economy/events",
            get(instance_economy_events),
        )
        // admin
        .route("/v1/admin/health", get(admin_health))
        .route("/v1/admin/audit", get(admin_audit))
        .route("/v1/admin/accounts/{id}", get(admin_account))
        .route("/v1/admin/accounts/{id}/ban", post(admin_ban))
        .route("/v1/admin/accounts/{id}/unban", post(admin_unban))
        .route("/v1/admin/accounts/{id}/kick", post(admin_kick))
        .route("/v1/admin/accounts/{id}/economy/adjust", post(admin_adjust))
        .route(
            "/v1/admin/flags/{key}",
            put(admin_set_flag).delete(admin_delete_flag),
        )
        .route("/v1/admin/leaderboards/reset", post(admin_reset_board))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            crate::cors::layer,
        ))
        .with_state(state)
}

// ------------------------------------------------------------------ extractors

/// JSON body whose parse errors come back as our JSON `bad_request` error.
pub struct Body<T>(pub T);

impl<S: Send + Sync, T: DeserializeOwned> FromRequest<S> for Body<T> {
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        Json::<T>::from_request(req, state)
            .await
            .map(|Json(v)| Body(v))
            .map_err(|e| ApiError::bad_request(e.body_text()))
    }
}

/// Client IP from the socket (`unknown` when served without connect info, e.g. in-process
/// tests). Not proxy-aware: put the service behind a proxy that rate-limits by real IP, or
/// extend this to trust `X-Forwarded-For` from known proxies.
pub struct ClientIp(pub String);

impl<S: Send + Sync> FromRequestParts<S> for ClientIp {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        Ok(ClientIp(
            parts
                .extensions
                .get::<ConnectInfo<SocketAddr>>()
                .map(|c| c.0.ip().to_string())
                .unwrap_or_else(|| "unknown".into()),
        ))
    }
}

/// An authenticated player.
pub struct Player(pub AccountId);

impl FromRequestParts<AppState> for Player {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, st: &AppState) -> Result<Self, Self::Rejection> {
        let token = bearer(&parts.headers).ok_or_else(ApiError::unauthorized)?;
        let id = auth::authenticate(&st.db.lock(), &token, st.now())?;
        Ok(Player(id))
    }
}

/// An authenticated admin; carries the actor name for the audit log (`X-Admin-Actor`).
pub struct Admin(pub String);

impl FromRequestParts<AppState> for Admin {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, st: &AppState) -> Result<Self, Self::Rejection> {
        let expected = &st.config.admin_token;
        if expected.is_empty() {
            return Err(ApiError::forbidden("admin_disabled", "admin API disabled"));
        }
        let token = bearer(&parts.headers).ok_or_else(ApiError::unauthorized)?;
        if !constant_time_eq(token.as_bytes(), expected.as_bytes()) {
            return Err(ApiError::unauthorized());
        }
        let actor = parts
            .headers
            .get("x-admin-actor")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.chars().take(64).collect::<String>())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "admin".into());
        Ok(Admin(actor))
    }
}

fn bearer(headers: &HeaderMap) -> Option<String> {
    headers
        .get(AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
        .map(|s| s.trim().to_string())
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn instance_auth(st: &AppState, headers: &HeaderMap, id: InstanceId) -> ApiResult<()> {
    let token = bearer(headers).ok_or_else(ApiError::unauthorized)?;
    directory::authenticate(&st.db.lock(), id, &token)
}

fn limit(st: &AppState, group: &'static str, key: &str, l: crate::config::Limit) -> ApiResult<()> {
    if st.limiter.check(group, key, l, st.now()) {
        Ok(())
    } else {
        Err(ApiError::rate_limited())
    }
}

// ------------------------------------------------------------------ misc

async fn healthz() -> &'static str {
    "ok"
}

async fn openapi() -> Response {
    ([(CONTENT_TYPE, "application/yaml")], OPENAPI_YAML).into_response()
}

async fn dashboard() -> Html<&'static str> {
    Html(DASHBOARD_HTML)
}

async fn public_flags(
    State(st): State<AppState>,
) -> ApiResult<Json<BTreeMap<String, serde_json::Value>>> {
    Ok(Json(directory::flags(&st.db.lock())?))
}

// ------------------------------------------------------------------ auth

async fn providers() -> Json<ProvidersResponse> {
    Json(ProvidersResponse {
        login: auth::LOGIN_PROVIDERS
            .iter()
            .map(|s| s.to_string())
            .collect(),
        linkable: auth::LINKABLE_PROVIDERS
            .iter()
            .map(|s| s.to_string())
            .collect(),
    })
}

/// Run CPU-heavy work (password hashing) off the async workers.
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> ApiResult<T> + Send + 'static,
) -> ApiResult<T> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| ApiError::internal(format!("worker: {e}")))?
}

async fn signup(
    State(st): State<AppState>,
    ClientIp(ip): ClientIp,
    Body(req): Body<SignupRequest>,
) -> ApiResult<(StatusCode, Json<SessionResponse>)> {
    limit(&st, "signup_ip", &ip, st.config.rate_limits.signup_per_ip)?;
    let email = auth::validate_signup(&req.email, &req.password, &req.display_name)?;
    let cost = st.config.hash_cost;
    let password = req.password;
    let phc = blocking(move || auth::hash_password(&password, cost)).await?;
    let now = st.now();
    let s = auth::create_account(
        &mut st.db.lock(),
        &st.economy,
        &email,
        &phc,
        &req.display_name,
        st.config.session_ttl_ms,
        now,
    )?;
    Ok((StatusCode::CREATED, Json(s)))
}

async fn login(
    State(st): State<AppState>,
    ClientIp(ip): ClientIp,
    Body(req): Body<LoginRequest>,
) -> ApiResult<Json<SessionResponse>> {
    limit(&st, "login_ip", &ip, st.config.rate_limits.login_per_ip)?;
    let email_key = req.email.trim().to_lowercase();
    limit(
        &st,
        "login_email",
        &email_key,
        st.config.rate_limits.login_per_email,
    )?;
    let identity = auth::password_identity(&st.db.lock(), &req.email)?;
    let cost = st.config.hash_cost;
    let password = req.password;
    let account =
        blocking(move || auth::check_password(&password, identity.as_ref(), cost)).await?;
    let now = st.now();
    Ok(Json(auth::open_session(
        &st.db.lock(),
        account,
        st.config.session_ttl_ms,
        now,
    )?))
}

async fn logout(
    State(st): State<AppState>,
    headers: HeaderMap,
    _p: Player,
) -> ApiResult<StatusCode> {
    let token = bearer(&headers).ok_or_else(ApiError::unauthorized)?;
    auth::logout(&st.db.lock(), &token)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn link(
    State(st): State<AppState>,
    Player(id): Player,
    Body(req): Body<LinkRequest>,
) -> ApiResult<StatusCode> {
    auth::request_link(&st.db.lock(), id, &req.provider, st.now())?;
    Ok(StatusCode::ACCEPTED)
}

// ------------------------------------------------------------------ player

async fn me(State(st): State<AppState>, Player(id): Player) -> ApiResult<Json<Profile>> {
    Ok(Json(auth::profile(&st.db.lock(), id)?))
}

async fn my_economy(
    State(st): State<AppState>,
    Player(id): Player,
) -> ApiResult<Json<wp_economy::PlayerAccount>> {
    store::load(&st.db.lock(), PlayerId(id))
        .map_err(store::store_error)?
        .map(Json)
        .ok_or_else(|| ApiError::not_found("economy account"))
}

/// `?from_seq=N`
#[derive(Debug, Deserialize)]
struct FromSeq {
    #[serde(default)]
    from_seq: u64,
}

async fn my_events(
    State(st): State<AppState>,
    Player(id): Player,
    Query(q): Query<FromSeq>,
) -> ApiResult<Json<Vec<wp_economy::EconomyEvent>>> {
    Ok(Json(
        store::events_since(&st.db.lock(), PlayerId(id), q.from_seq).map_err(store::store_error)?,
    ))
}

/// `?period=all|daily|weekly&limit=N`
#[derive(Debug, Deserialize)]
struct BoardQuery {
    period: Option<String>,
    limit: Option<u32>,
}

async fn leaderboard(
    State(st): State<AppState>,
    Path(board): Path<String>,
    Query(q): Query<BoardQuery>,
) -> ApiResult<Json<Leaderboard>> {
    let period = Period::parse(q.period.as_deref().unwrap_or("all"))?;
    Ok(Json(leaderboards::top(
        &st.db.lock(),
        &board,
        period,
        q.limit.unwrap_or(50),
        st.now(),
    )?))
}

// ------------------------------------------------------------------ parties & crews

async fn create_party(
    State(st): State<AppState>,
    Player(id): Player,
) -> ApiResult<(StatusCode, Json<PartyCreated>)> {
    let now = st.now();
    let p = matchmaking::create_party(&mut st.db.lock(), id, now)?;
    Ok((StatusCode::CREATED, Json(p)))
}

async fn join_party(
    State(st): State<AppState>,
    Player(id): Player,
    Path(party): Path<u32>,
    Body(req): Body<JoinPartyRequest>,
) -> ApiResult<Json<PartyInfo>> {
    let now = st.now();
    Ok(Json(matchmaking::join_party(
        &mut st.db.lock(),
        id,
        party,
        &req.code,
        now,
    )?))
}

async fn leave_party(State(st): State<AppState>, Player(id): Player) -> ApiResult<StatusCode> {
    matchmaking::leave_party(&mut st.db.lock(), id)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn get_party(
    State(st): State<AppState>,
    Player(id): Player,
    Path(party): Path<u32>,
) -> ApiResult<Json<PartyInfo>> {
    Ok(Json(matchmaking::party(&st.db.lock(), id, party)?))
}

async fn create_crew(
    State(st): State<AppState>,
    Player(id): Player,
    Body(req): Body<CreateCrewRequest>,
) -> ApiResult<(StatusCode, Json<CrewInfo>)> {
    let now = st.now();
    let c = crews::create(&mut st.db.lock(), id, &req.name, &req.tag, now)?;
    Ok((StatusCode::CREATED, Json(c)))
}

async fn join_crew(
    State(st): State<AppState>,
    Player(id): Player,
    Path(crew): Path<u32>,
) -> ApiResult<Json<CrewInfo>> {
    Ok(Json(crews::join(&st.db.lock(), id, crew, st.now())?))
}

async fn leave_crew(State(st): State<AppState>, Player(id): Player) -> ApiResult<StatusCode> {
    crews::leave(&mut st.db.lock(), id)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn get_crew(State(st): State<AppState>, Path(crew): Path<u32>) -> ApiResult<Json<CrewInfo>> {
    Ok(Json(crews::get(&st.db.lock(), crew)?))
}

// ------------------------------------------------------------------ matchmaking

async fn mm_queue(
    State(st): State<AppState>,
    Player(id): Player,
    Body(req): Body<QueueRequest>,
) -> ApiResult<Json<TicketStatus>> {
    limit(
        &st,
        "mm",
        &id.to_string(),
        st.config.rate_limits.matchmaking_per_account,
    )?;
    let now = st.now();
    Ok(Json(matchmaking::queue(
        &mut st.db.lock(),
        id,
        &req.prefs,
        &st.config,
        now,
    )?))
}

async fn mm_status(
    State(st): State<AppState>,
    Player(id): Player,
) -> ApiResult<Json<TicketStatus>> {
    limit(
        &st,
        "mm",
        &id.to_string(),
        st.config.rate_limits.matchmaking_per_account,
    )?;
    let now = st.now();
    let mut conn = st.db.lock();
    matchmaking::process(&mut conn, &st.config, now)?;
    Ok(Json(matchmaking::status(&conn, id, &st.config, now)?))
}

async fn mm_cancel(State(st): State<AppState>, Player(id): Player) -> ApiResult<StatusCode> {
    matchmaking::cancel(&st.db.lock(), id)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn mm_reconnect(
    State(st): State<AppState>,
    Player(id): Player,
    Body(req): Body<ReconnectRequest>,
) -> ApiResult<Json<TicketStatus>> {
    let now = st.now();
    Ok(Json(matchmaking::reconnect(
        &mut st.db.lock(),
        id,
        &req.reconnect_token,
        &st.config,
        now,
    )?))
}

// ------------------------------------------------------------------ instances

async fn list_instances(State(st): State<AppState>) -> ApiResult<Json<Vec<InstanceInfo>>> {
    let now = st.now();
    let conn = st.db.lock();
    directory::refresh_liveness(&conn, st.config.heartbeat_interval_ms, now)?;
    Ok(Json(directory::list(&conn, false, now)?))
}

async fn register_instance(
    State(st): State<AppState>,
    headers: HeaderMap,
    Body(req): Body<RegisterInstanceRequest>,
) -> ApiResult<(StatusCode, Json<RegisterInstanceResponse>)> {
    let key = bearer(&headers).ok_or_else(ApiError::unauthorized)?;
    if st.config.instance_key.is_empty()
        || !constant_time_eq(key.as_bytes(), st.config.instance_key.as_bytes())
    {
        return Err(ApiError::unauthorized());
    }
    let now = st.now();
    let mut conn = st.db.lock();
    let r = directory::register(&conn, &req, st.config.heartbeat_interval_ms, now)?;
    matchmaking::process(&mut conn, &st.config, now)?;
    Ok((StatusCode::CREATED, Json(r)))
}

async fn deregister_instance(
    State(st): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<InstanceId>,
) -> ApiResult<StatusCode> {
    instance_auth(&st, &headers, id)?;
    let now = st.now();
    let mut conn = st.db.lock();
    directory::deregister(&conn, id)?;
    matchmaking::process(&mut conn, &st.config, now)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn heartbeat(
    State(st): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<InstanceId>,
    Body(req): Body<HeartbeatRequest>,
) -> ApiResult<Json<HeartbeatResponse>> {
    instance_auth(&st, &headers, id)?;
    let now = st.now();
    let mut conn = st.db.lock();
    let resp = directory::heartbeat(&conn, id, &req, st.config.reconnect_ttl_ms, now)?;
    matchmaking::process(&mut conn, &st.config, now)?;
    Ok(Json(resp))
}

async fn verify_join(
    State(st): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<InstanceId>,
    Body(req): Body<VerifyJoinRequest>,
) -> ApiResult<Json<VerifyJoinResponse>> {
    instance_auth(&st, &headers, id)?;
    let now = st.now();
    Ok(Json(matchmaking::verify_join(
        &st.db.lock(),
        id,
        &req.join_token,
        &st.config,
        now,
    )?))
}

async fn report_race(
    State(st): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<InstanceId>,
    Body(req): Body<RaceReport>,
) -> ApiResult<Json<RaceReportResponse>> {
    instance_auth(&st, &headers, id)?;
    let now = st.now();
    Ok(Json(results::report_race(
        &mut st.db.lock(),
        id,
        &req,
        now,
    )?))
}

async fn report_pursuit(
    State(st): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<InstanceId>,
    Body(req): Body<PursuitReport>,
) -> ApiResult<Json<PursuitReportResponse>> {
    instance_auth(&st, &headers, id)?;
    let now = st.now();
    Ok(Json(results::report_pursuit(
        &mut st.db.lock(),
        id,
        &req,
        now,
    )?))
}

async fn report_territory(
    State(st): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<InstanceId>,
    Body(req): Body<TerritoryReport>,
) -> ApiResult<StatusCode> {
    instance_auth(&st, &headers, id)?;
    crews::set_territory(&st.db.lock(), id, &req.district_id, req.crew_id, st.now())?;
    Ok(StatusCode::NO_CONTENT)
}

async fn instance_load_economy(
    State(st): State<AppState>,
    headers: HeaderMap,
    Path((id, account)): Path<(InstanceId, AccountId)>,
) -> ApiResult<Json<wp_economy::PlayerAccount>> {
    instance_auth(&st, &headers, id)?;
    let conn = st.db.lock();
    results::ensure_hosted(&conn, id, account)?;
    store::load(&conn, PlayerId(account))
        .map_err(store::store_error)?
        .map(Json)
        .ok_or_else(|| ApiError::not_found("economy account"))
}

async fn instance_commit_economy(
    State(st): State<AppState>,
    headers: HeaderMap,
    Path((id, account)): Path<(InstanceId, AccountId)>,
    Body(req): Body<CommitRequest>,
) -> ApiResult<StatusCode> {
    instance_auth(&st, &headers, id)?;
    if req.account.player != PlayerId(account) {
        return Err(ApiError::bad_request(
            "account.player does not match the path",
        ));
    }
    let now = st.now();
    let mut conn = st.db.lock();
    results::ensure_hosted(&conn, id, account)?;
    store::commit(&mut conn, &req.account, &req.events, now).map_err(store::store_error)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn instance_economy_events(
    State(st): State<AppState>,
    headers: HeaderMap,
    Path((id, account)): Path<(InstanceId, AccountId)>,
    Query(q): Query<FromSeq>,
) -> ApiResult<Json<Vec<wp_economy::EconomyEvent>>> {
    instance_auth(&st, &headers, id)?;
    let conn = st.db.lock();
    results::ensure_hosted(&conn, id, account)?;
    Ok(Json(
        store::events_since(&conn, PlayerId(account), q.from_seq).map_err(store::store_error)?,
    ))
}

// ------------------------------------------------------------------ admin

async fn admin_health(State(st): State<AppState>, _a: Admin) -> ApiResult<Json<HealthReport>> {
    Ok(Json(admin::health(
        &st.db.lock(),
        st.config.heartbeat_interval_ms,
        st.now(),
    )?))
}

/// `?limit=N`
#[derive(Debug, Deserialize)]
struct LimitQuery {
    limit: Option<u32>,
}

async fn admin_audit(
    State(st): State<AppState>,
    _a: Admin,
    Query(q): Query<LimitQuery>,
) -> ApiResult<Json<Vec<AuditEntry>>> {
    Ok(Json(admin::audit_log(
        &st.db.lock(),
        q.limit.unwrap_or(100),
    )?))
}

async fn admin_account(
    State(st): State<AppState>,
    _a: Admin,
    Path(id): Path<AccountId>,
) -> ApiResult<Json<AdminAccountView>> {
    Ok(Json(admin::account_view(&st.db.lock(), id)?))
}

async fn admin_ban(
    State(st): State<AppState>,
    Admin(actor): Admin,
    Path(id): Path<AccountId>,
    Body(req): Body<BanRequest>,
) -> ApiResult<Json<AdminAccountView>> {
    let now = st.now();
    let mut conn = st.db.lock();
    admin::ban(&mut conn, &actor, id, &req.reason, req.duration_s, now)?;
    Ok(Json(admin::account_view(&conn, id)?))
}

async fn admin_unban(
    State(st): State<AppState>,
    Admin(actor): Admin,
    Path(id): Path<AccountId>,
) -> ApiResult<Json<AdminAccountView>> {
    let now = st.now();
    let mut conn = st.db.lock();
    admin::unban(&mut conn, &actor, id, now)?;
    Ok(Json(admin::account_view(&conn, id)?))
}

async fn admin_kick(
    State(st): State<AppState>,
    Admin(actor): Admin,
    Path(id): Path<AccountId>,
    Body(req): Body<KickRequest>,
) -> ApiResult<StatusCode> {
    let now = st.now();
    admin::kick(&mut st.db.lock(), &actor, id, &req.reason, now)?;
    Ok(StatusCode::ACCEPTED)
}

async fn admin_adjust(
    State(st): State<AppState>,
    Admin(actor): Admin,
    Path(id): Path<AccountId>,
    Body(req): Body<AdjustRequest>,
) -> ApiResult<Json<wp_economy::PlayerAccount>> {
    let now = st.now();
    Ok(Json(admin::adjust(
        &mut st.db.lock(),
        &st.economy,
        &actor,
        id,
        req.cash,
        &req.note,
        now,
    )?))
}

async fn admin_set_flag(
    State(st): State<AppState>,
    Admin(actor): Admin,
    Path(key): Path<String>,
    Body(req): Body<SetFlagRequest>,
) -> ApiResult<StatusCode> {
    let now = st.now();
    admin::set_flag(&mut st.db.lock(), &actor, &key, &req.value, now)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn admin_delete_flag(
    State(st): State<AppState>,
    Admin(actor): Admin,
    Path(key): Path<String>,
) -> ApiResult<StatusCode> {
    let now = st.now();
    admin::delete_flag(&mut st.db.lock(), &actor, &key, now)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn admin_reset_board(
    State(st): State<AppState>,
    Admin(actor): Admin,
    Body(req): Body<ResetBoardRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    let period = Period::parse(&req.period)?;
    let now = st.now();
    let n = admin::reset_board(&mut st.db.lock(), &actor, &req.board, period, now)?;
    Ok(Json(serde_json::json!({ "removed": n })))
}

/// Every `(method, path)` the router serves; `tests/openapi.rs` checks each is documented.
pub const ROUTES: &[(&str, &str)] = &[
    ("get", "/healthz"),
    ("get", "/openapi.yaml"),
    ("get", "/admin"),
    ("get", "/v1/auth/providers"),
    ("post", "/v1/auth/signup"),
    ("post", "/v1/auth/login"),
    ("post", "/v1/auth/logout"),
    ("post", "/v1/auth/link"),
    ("get", "/v1/me"),
    ("get", "/v1/me/economy"),
    ("get", "/v1/me/events"),
    ("get", "/v1/flags"),
    ("get", "/v1/leaderboards/{board}"),
    ("post", "/v1/parties"),
    ("post", "/v1/parties/leave"),
    ("get", "/v1/parties/{id}"),
    ("post", "/v1/parties/{id}/join"),
    ("post", "/v1/crews"),
    ("post", "/v1/crews/leave"),
    ("get", "/v1/crews/{id}"),
    ("post", "/v1/crews/{id}/join"),
    ("post", "/v1/matchmaking/queue"),
    ("delete", "/v1/matchmaking/queue"),
    ("get", "/v1/matchmaking/status"),
    ("post", "/v1/matchmaking/reconnect"),
    ("get", "/v1/instances"),
    ("post", "/v1/instances"),
    ("delete", "/v1/instances/{id}"),
    ("post", "/v1/instances/{id}/heartbeat"),
    ("post", "/v1/instances/{id}/join/verify"),
    ("post", "/v1/instances/{id}/results/race"),
    ("post", "/v1/instances/{id}/results/pursuit"),
    ("post", "/v1/instances/{id}/territory"),
    ("get", "/v1/instances/{id}/accounts/{account}/economy"),
    (
        "post",
        "/v1/instances/{id}/accounts/{account}/economy/commit",
    ),
    (
        "get",
        "/v1/instances/{id}/accounts/{account}/economy/events",
    ),
    ("get", "/v1/admin/health"),
    ("get", "/v1/admin/audit"),
    ("get", "/v1/admin/accounts/{id}"),
    ("post", "/v1/admin/accounts/{id}/ban"),
    ("post", "/v1/admin/accounts/{id}/unban"),
    ("post", "/v1/admin/accounts/{id}/kick"),
    ("post", "/v1/admin/accounts/{id}/economy/adjust"),
    ("put", "/v1/admin/flags/{key}"),
    ("delete", "/v1/admin/flags/{key}"),
    ("post", "/v1/admin/leaderboards/reset"),
];
