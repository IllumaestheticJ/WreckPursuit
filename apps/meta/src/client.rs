//! Blocking HTTP client for the meta API (`ureq`, no TLS — terminate TLS at a proxy).
//! Used by `tools/meta-cli`, the integration tests and, later, `apps/server`
//! (see `docs/integration/meta.md`).

use std::collections::BTreeMap;
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::Serialize;
use wp_contracts::PlayerId;
use wp_economy::{AccountStore, EconomyEvent, PlayerAccount, StoreError};

use crate::api::*;
use crate::error::ErrorBody;

/// A failed call.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ClientError {
    /// The service answered with an error status.
    #[error("HTTP {status} {}: {}", body.error, body.message)]
    Api {
        /// HTTP status.
        status: u16,
        /// Error body.
        body: ErrorBody,
    },
    /// Network / decoding failure.
    #[error("transport: {0}")]
    Transport(String),
}

impl ClientError {
    /// HTTP status, if the service answered.
    pub fn status(&self) -> Option<u16> {
        match self {
            Self::Api { status, .. } => Some(*status),
            Self::Transport(_) => None,
        }
    }

    /// Error code, if the service answered.
    pub fn code(&self) -> Option<&str> {
        match self {
            Self::Api { body, .. } => Some(&body.error),
            Self::Transport(_) => None,
        }
    }
}

/// Result alias.
pub type ClientResult<T> = Result<T, ClientError>;

/// Client for one base URL with an optional bearer token (player session, instance token or
/// admin token — whichever the endpoints you call need).
#[derive(Clone)]
pub struct MetaClient {
    base: String,
    token: Option<String>,
    admin_actor: Option<String>,
    agent: ureq::Agent,
}

impl MetaClient {
    /// Client for `base` (e.g. `http://127.0.0.1:8090`), no token.
    pub fn new(base: &str) -> Self {
        Self {
            base: base.trim_end_matches('/').to_string(),
            token: None,
            admin_actor: None,
            agent: ureq::AgentBuilder::new()
                .timeout(Duration::from_secs(15))
                .build(),
        }
    }

    /// Same client with a bearer token.
    pub fn with_token(&self, token: &str) -> Self {
        Self {
            token: Some(token.to_string()),
            ..self.clone()
        }
    }

    /// Same client sending `X-Admin-Actor` (audit log name) on admin calls.
    pub fn with_admin_actor(&self, actor: &str) -> Self {
        Self {
            admin_actor: Some(actor.to_string()),
            ..self.clone()
        }
    }

    /// Base URL.
    pub fn base(&self) -> &str {
        &self.base
    }

    fn request(&self, method: &str, path: &str) -> ureq::Request {
        let mut r = self
            .agent
            .request(method, &format!("{}{}", self.base, path));
        if let Some(t) = &self.token {
            r = r.set("Authorization", &format!("Bearer {t}"));
        }
        if let Some(a) = &self.admin_actor {
            r = r.set("X-Admin-Actor", a);
        }
        r
    }

    fn send(
        &self,
        method: &str,
        path: &str,
        body: Option<&serde_json::Value>,
    ) -> ClientResult<ureq::Response> {
        let req = self.request(method, path);
        let res = match body {
            Some(b) => req.send_json(b),
            None => req.call(),
        };
        match res {
            Ok(r) => Ok(r),
            Err(ureq::Error::Status(status, r)) => {
                let text = r.into_string().unwrap_or_default();
                let body = serde_json::from_str(&text).unwrap_or(ErrorBody {
                    error: "http".into(),
                    message: text,
                });
                Err(ClientError::Api { status, body })
            }
            Err(e) => Err(ClientError::Transport(e.to_string())),
        }
    }

    /// Call and decode a JSON response.
    pub fn call<R: DeserializeOwned>(
        &self,
        method: &str,
        path: &str,
        body: Option<&impl Serialize>,
    ) -> ClientResult<R> {
        let body = body
            .map(serde_json::to_value)
            .transpose()
            .map_err(|e| ClientError::Transport(e.to_string()))?;
        self.send(method, path, body.as_ref())?
            .into_json()
            .map_err(|e| ClientError::Transport(e.to_string()))
    }

    /// Call an endpoint that answers with no body.
    pub fn call_empty(
        &self,
        method: &str,
        path: &str,
        body: Option<&impl Serialize>,
    ) -> ClientResult<()> {
        let body = body
            .map(serde_json::to_value)
            .transpose()
            .map_err(|e| ClientError::Transport(e.to_string()))?;
        self.send(method, path, body.as_ref()).map(|_| ())
    }

    const NONE: Option<&'static ()> = None;

    // ---------------------------------------------------------- player

    /// Create an account.
    pub fn signup(
        &self,
        email: &str,
        password: &str,
        display_name: &str,
    ) -> ClientResult<SessionResponse> {
        self.call(
            "POST",
            "/v1/auth/signup",
            Some(&SignupRequest {
                email: email.into(),
                password: password.into(),
                display_name: display_name.into(),
            }),
        )
    }

    /// Log in.
    pub fn login(&self, email: &str, password: &str) -> ClientResult<SessionResponse> {
        self.call(
            "POST",
            "/v1/auth/login",
            Some(&LoginRequest {
                email: email.into(),
                password: password.into(),
            }),
        )
    }

    /// Revoke the current session.
    pub fn logout(&self) -> ClientResult<()> {
        self.call_empty("POST", "/v1/auth/logout", Self::NONE)
    }

    /// Own profile.
    pub fn me(&self) -> ClientResult<Profile> {
        self.call("GET", "/v1/me", Self::NONE)
    }

    /// Own economy account.
    pub fn my_economy(&self) -> ClientResult<PlayerAccount> {
        self.call("GET", "/v1/me/economy", Self::NONE)
    }

    /// Queue for a match.
    pub fn queue(&self, prefs: &MatchPrefs) -> ClientResult<TicketStatus> {
        self.call(
            "POST",
            "/v1/matchmaking/queue",
            Some(&QueueRequest {
                prefs: prefs.clone(),
            }),
        )
    }

    /// Poll the ticket (mints a fresh join token when assigned).
    pub fn ticket_status(&self) -> ClientResult<TicketStatus> {
        self.call("GET", "/v1/matchmaking/status", Self::NONE)
    }

    /// Leave the queue.
    pub fn cancel_queue(&self) -> ClientResult<()> {
        self.call_empty("DELETE", "/v1/matchmaking/queue", Self::NONE)
    }

    /// Rejoin with a reconnect token.
    pub fn reconnect(&self, reconnect_token: &str) -> ClientResult<TicketStatus> {
        self.call(
            "POST",
            "/v1/matchmaking/reconnect",
            Some(&ReconnectRequest {
                reconnect_token: reconnect_token.into(),
            }),
        )
    }

    /// Create a party.
    pub fn create_party(&self) -> ClientResult<PartyCreated> {
        self.call("POST", "/v1/parties", Self::NONE)
    }

    /// Join a party.
    pub fn join_party(&self, party_id: u32, code: &str) -> ClientResult<PartyInfo> {
        self.call(
            "POST",
            &format!("/v1/parties/{party_id}/join"),
            Some(&JoinPartyRequest { code: code.into() }),
        )
    }

    /// A leaderboard (`period`: `all`, `daily`, `weekly`).
    pub fn leaderboard(&self, board: &str, period: &str, limit: u32) -> ClientResult<Leaderboard> {
        self.call(
            "GET",
            &format!("/v1/leaderboards/{board}?period={period}&limit={limit}"),
            Self::NONE,
        )
    }

    /// Feature flags.
    pub fn flags(&self) -> ClientResult<BTreeMap<String, serde_json::Value>> {
        self.call("GET", "/v1/flags", Self::NONE)
    }

    /// Live instances.
    pub fn instances(&self) -> ClientResult<Vec<InstanceInfo>> {
        self.call("GET", "/v1/instances", Self::NONE)
    }

    // ---------------------------------------------------------- instance (apps/server)

    /// Register an instance (token = instance registration key).
    pub fn register_instance(
        &self,
        req: &RegisterInstanceRequest,
    ) -> ClientResult<RegisterInstanceResponse> {
        self.call("POST", "/v1/instances", Some(req))
    }

    /// Heartbeat (token = instance token).
    pub fn heartbeat(
        &self,
        id: InstanceId,
        req: &HeartbeatRequest,
    ) -> ClientResult<HeartbeatResponse> {
        self.call("POST", &format!("/v1/instances/{id}/heartbeat"), Some(req))
    }

    /// Deregister.
    pub fn deregister(&self, id: InstanceId) -> ClientResult<()> {
        self.call_empty("DELETE", &format!("/v1/instances/{id}"), Self::NONE)
    }

    /// Verify a client's join token.
    pub fn verify_join(
        &self,
        id: InstanceId,
        join_token: &str,
    ) -> ClientResult<VerifyJoinResponse> {
        self.call(
            "POST",
            &format!("/v1/instances/{id}/join/verify"),
            Some(&VerifyJoinRequest {
                join_token: join_token.into(),
            }),
        )
    }

    /// Report a race.
    pub fn report_race(
        &self,
        id: InstanceId,
        report: &RaceReport,
    ) -> ClientResult<RaceReportResponse> {
        self.call(
            "POST",
            &format!("/v1/instances/{id}/results/race"),
            Some(report),
        )
    }

    /// Report a pursuit.
    pub fn report_pursuit(
        &self,
        id: InstanceId,
        report: &PursuitReport,
    ) -> ClientResult<PursuitReportResponse> {
        self.call(
            "POST",
            &format!("/v1/instances/{id}/results/pursuit"),
            Some(report),
        )
    }

    /// Report a territory change.
    pub fn report_territory(&self, id: InstanceId, report: &TerritoryReport) -> ClientResult<()> {
        self.call_empty(
            "POST",
            &format!("/v1/instances/{id}/territory"),
            Some(report),
        )
    }

    /// Load a hosted player's economy account (`Ok(None)` if it has none).
    pub fn load_economy(
        &self,
        id: InstanceId,
        account: AccountId,
    ) -> ClientResult<Option<PlayerAccount>> {
        match self.call(
            "GET",
            &format!("/v1/instances/{id}/accounts/{account}/economy"),
            Self::NONE,
        ) {
            Ok(a) => Ok(Some(a)),
            Err(e) if e.code() == Some("not_found") => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Commit a hosted player's economy change.
    pub fn commit_economy(
        &self,
        id: InstanceId,
        account: &PlayerAccount,
        events: &[EconomyEvent],
    ) -> ClientResult<()> {
        self.call_empty(
            "POST",
            &format!(
                "/v1/instances/{id}/accounts/{}/economy/commit",
                account.player.0
            ),
            Some(&CommitRequest {
                account: account.clone(),
                events: events.to_vec(),
            }),
        )
    }

    /// A hosted player's economy events from `from_seq`.
    pub fn economy_events(
        &self,
        id: InstanceId,
        account: AccountId,
        from_seq: u64,
    ) -> ClientResult<Vec<EconomyEvent>> {
        self.call(
            "GET",
            &format!("/v1/instances/{id}/accounts/{account}/economy/events?from_seq={from_seq}"),
            Self::NONE,
        )
    }

    // ---------------------------------------------------------- admin

    /// Health report.
    pub fn admin_health(&self) -> ClientResult<HealthReport> {
        self.call("GET", "/v1/admin/health", Self::NONE)
    }

    /// Account view.
    pub fn admin_account(&self, id: AccountId) -> ClientResult<AdminAccountView> {
        self.call("GET", &format!("/v1/admin/accounts/{id}"), Self::NONE)
    }

    /// Ban (`duration_s: None` = permanent).
    pub fn admin_ban(
        &self,
        id: AccountId,
        reason: &str,
        duration_s: Option<i64>,
    ) -> ClientResult<AdminAccountView> {
        self.call(
            "POST",
            &format!("/v1/admin/accounts/{id}/ban"),
            Some(&BanRequest {
                reason: reason.into(),
                duration_s,
            }),
        )
    }

    /// Unban.
    pub fn admin_unban(&self, id: AccountId) -> ClientResult<AdminAccountView> {
        self.call(
            "POST",
            &format!("/v1/admin/accounts/{id}/unban"),
            Self::NONE,
        )
    }

    /// Kick.
    pub fn admin_kick(&self, id: AccountId, reason: &str) -> ClientResult<()> {
        self.call_empty(
            "POST",
            &format!("/v1/admin/accounts/{id}/kick"),
            Some(&KickRequest {
                reason: reason.into(),
            }),
        )
    }

    /// Economy adjustment.
    pub fn admin_adjust(
        &self,
        id: AccountId,
        cash: i64,
        note: &str,
    ) -> ClientResult<PlayerAccount> {
        self.call(
            "POST",
            &format!("/v1/admin/accounts/{id}/economy/adjust"),
            Some(&AdjustRequest {
                cash,
                note: note.into(),
            }),
        )
    }

    /// Set a flag.
    pub fn admin_set_flag(&self, key: &str, value: serde_json::Value) -> ClientResult<()> {
        self.call_empty(
            "PUT",
            &format!("/v1/admin/flags/{key}"),
            Some(&SetFlagRequest { value }),
        )
    }

    /// Delete a flag.
    pub fn admin_delete_flag(&self, key: &str) -> ClientResult<()> {
        self.call_empty("DELETE", &format!("/v1/admin/flags/{key}"), Self::NONE)
    }

    /// Audit log, newest first.
    pub fn admin_audit(&self, limit: u32) -> ClientResult<Vec<AuditEntry>> {
        self.call("GET", &format!("/v1/admin/audit?limit={limit}"), Self::NONE)
    }

    /// Reset the current period of a board.
    pub fn admin_reset_board(&self, board: &str, period: &str) -> ClientResult<serde_json::Value> {
        self.call(
            "POST",
            "/v1/admin/leaderboards/reset",
            Some(&ResetBoardRequest {
                board: board.into(),
                period: period.into(),
            }),
        )
    }
}

/// [`AccountStore`] over HTTP for an instance server: drop-in for `InMemoryStore` /
/// `SqliteStore` in `apps/server`. Only accounts that joined this instance are reachable.
pub struct RemoteAccountStore {
    client: MetaClient,
    instance: InstanceId,
}

impl RemoteAccountStore {
    /// `client` must carry the instance token.
    pub fn new(client: MetaClient, instance: InstanceId) -> Self {
        Self { client, instance }
    }
}

fn to_store_error(e: ClientError) -> StoreError {
    if e.code() == Some("seq_conflict") {
        // The message is the server-side StoreError text: "... expected seq A, got B".
        let msg = match &e {
            ClientError::Api { body, .. } => body.message.clone(),
            ClientError::Transport(_) => String::new(),
        };
        let nums: Vec<u64> = msg
            .split(|c: char| !c.is_ascii_digit())
            .filter_map(|s| s.parse().ok())
            .collect();
        return StoreError::SeqConflict {
            expected: nums.first().copied().unwrap_or(0),
            got: nums.get(1).copied().unwrap_or(0),
        };
    }
    StoreError::Backend(e.to_string())
}

impl AccountStore for RemoteAccountStore {
    fn load(&self, player: PlayerId) -> Result<Option<PlayerAccount>, StoreError> {
        self.client
            .load_economy(self.instance, player.0)
            .map_err(to_store_error)
    }

    fn commit(
        &mut self,
        account: &PlayerAccount,
        events: &[EconomyEvent],
    ) -> Result<(), StoreError> {
        self.client
            .commit_economy(self.instance, account, events)
            .map_err(to_store_error)
    }

    fn events_since(
        &self,
        player: PlayerId,
        from_seq: u64,
    ) -> Result<Vec<EconomyEvent>, StoreError> {
        self.client
            .economy_events(self.instance, player.0, from_seq)
            .map_err(to_store_error)
    }
}
