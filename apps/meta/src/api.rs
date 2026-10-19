//! Request/response bodies of the HTTP API (`openapi.yaml` documents the same shapes).
//! Shared by the service, [`crate::client::MetaClient`], `tools/meta-cli` and `apps/server`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use wp_contracts::{DamageMode, Role, WantedLevel};

/// Durable account id. Equals the economy `PlayerId.0` until the contract gains a separate
/// `AccountId` (contract change request MC-1 in `docs/integration/meta.md`).
pub type AccountId = u32;
/// Instance id allocated at registration.
pub type InstanceId = u32;

// ---------------------------------------------------------------- auth

/// `POST /v1/auth/signup`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignupRequest {
    /// Login email (case-insensitive).
    pub email: String,
    /// Password, 8..=128 chars.
    pub password: String,
    /// Public name, 3..=24 chars of `[A-Za-z0-9_-]`, unique.
    pub display_name: String,
}

/// `POST /v1/auth/login`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginRequest {
    /// Login email.
    pub email: String,
    /// Password.
    pub password: String,
}

/// Response of signup and login.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionResponse {
    /// The account.
    pub account_id: AccountId,
    /// Bearer token for player endpoints.
    pub token: String,
    /// Token expiry (Unix ms).
    pub expires_ms: i64,
}

/// `POST /v1/auth/link` (stub).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkRequest {
    /// OAuth provider name, e.g. `steam`.
    pub provider: String,
}

/// `GET /v1/auth/providers`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProvidersResponse {
    /// Login providers this deployment accepts.
    pub login: Vec<String>,
    /// Providers that can be linked to an existing account.
    pub linkable: Vec<String>,
}

/// `GET /v1/me`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    /// Account id.
    pub account_id: AccountId,
    /// Public name.
    pub display_name: String,
    /// Creation time (Unix ms).
    pub created_ms: i64,
    /// Crew membership, if any.
    pub crew_id: Option<u32>,
    /// Party membership, if any.
    pub party_id: Option<u32>,
    /// Clean-race rating.
    pub clean_rating: f64,
}

// ---------------------------------------------------------------- directory

/// Players per role (instance capacity or population).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoleCounts {
    /// Street racers.
    pub racer: u32,
    /// Player-driven chase units.
    pub police_chase: u32,
    /// Police captain (0 or 1).
    pub police_captain: u32,
    /// Spectators.
    pub spectator: u32,
}

impl RoleCounts {
    /// Count for one role.
    pub fn get(&self, role: Role) -> u32 {
        match role {
            Role::Racer => self.racer,
            Role::PoliceChase => self.police_chase,
            Role::PoliceCaptain => self.police_captain,
            Role::Spectator => self.spectator,
        }
    }

    /// Mutable count for one role.
    pub fn get_mut(&mut self, role: Role) -> &mut u32 {
        match role {
            Role::Racer => &mut self.racer,
            Role::PoliceChase => &mut self.police_chase,
            Role::PoliceCaptain => &mut self.police_captain,
            Role::Spectator => &mut self.spectator,
        }
    }

    /// Sum over all roles.
    pub fn total(&self) -> u32 {
        self.racer + self.police_chase + self.police_captain + self.spectator
    }
}

/// `POST /v1/instances` (requires the instance registration key).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterInstanceRequest {
    /// Human-readable name.
    pub name: String,
    /// Region code, e.g. `eu-west`.
    pub region: String,
    /// City id (`data/city/...`).
    pub city: String,
    /// Damage mode the instance runs.
    pub mode: DamageMode,
    /// Class bracket (`D`, `C`, `B`, `A`, `S` or `open`).
    pub bracket: String,
    /// Address clients connect to, e.g. `ws://10.0.0.5:8080`.
    pub address: String,
    /// Maximum players per role (role ratio).
    pub slots: RoleCounts,
}

/// Registration result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegisterInstanceResponse {
    /// Allocated instance id.
    pub instance_id: InstanceId,
    /// Bearer token for all `/v1/instances/{id}/...` calls.
    pub instance_token: String,
    /// Send a heartbeat at least this often; 3 missed intervals mark the instance down.
    pub heartbeat_interval_ms: i64,
}

/// Instance lifecycle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstanceState {
    /// Accepting players.
    Up,
    /// Running, not accepting new players.
    Draining,
    /// Missed heartbeats or deregistered.
    Down,
}

impl InstanceState {
    /// Database string.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Up => "up",
            Self::Draining => "draining",
            Self::Down => "down",
        }
    }

    /// Parse the database string.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "up" => Some(Self::Up),
            "draining" => Some(Self::Draining),
            "down" => Some(Self::Down),
            _ => None,
        }
    }
}

/// `POST /v1/instances/{id}/heartbeat`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HeartbeatRequest {
    /// Current players per role.
    pub population: RoleCounts,
    /// Accounts currently connected (refreshes their reconnect tokens).
    #[serde(default)]
    pub players: Vec<AccountId>,
    /// Set to stop receiving new players.
    #[serde(default)]
    pub draining: bool,
    /// Mean server tick time over the last interval, ms (dashboard only).
    #[serde(default)]
    pub tick_ms: f32,
}

/// An admin kick the instance must carry out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KickOrder {
    /// Account to disconnect.
    pub account_id: AccountId,
    /// Reason to show the player.
    pub reason: String,
}

/// Heartbeat response: pending kicks and the current feature flags.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HeartbeatResponse {
    /// Players to disconnect now.
    pub kicks: Vec<KickOrder>,
    /// All feature flags.
    pub flags: BTreeMap<String, serde_json::Value>,
}

/// Public directory entry (`GET /v1/instances`, admin health).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InstanceInfo {
    /// Id.
    pub instance_id: InstanceId,
    /// Name.
    pub name: String,
    /// Region.
    pub region: String,
    /// City.
    pub city: String,
    /// Damage mode.
    pub mode: DamageMode,
    /// Class bracket.
    pub bracket: String,
    /// Connect address.
    pub address: String,
    /// Capacity per role.
    pub slots: RoleCounts,
    /// Reported population per role.
    pub population: RoleCounts,
    /// Seats held by matched players the reported population does not cover yet (assigned,
    /// or joined but not yet listed in a heartbeat).
    pub reserved: RoleCounts,
    /// State.
    pub state: InstanceState,
    /// Last heartbeat (Unix ms).
    pub last_heartbeat_ms: i64,
    /// Mean tick time last reported, ms.
    pub tick_ms: f32,
}

// ---------------------------------------------------------------- matchmaking

/// What the player wants to play.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatchPrefs {
    /// Racer, chase cop, captain or spectator.
    pub role: Role,
    /// Arcade or Simulation.
    pub mode: DamageMode,
    /// Class bracket (`D`..`S` or `open`). Instances with bracket `open` accept every bracket.
    pub bracket: String,
    /// Preferred region.
    pub region: String,
    /// Optional city filter.
    #[serde(default)]
    pub city: Option<String>,
}

/// `POST /v1/matchmaking/queue`. A party leader queues the whole party.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueRequest {
    /// Preferences.
    pub prefs: MatchPrefs,
}

/// Ticket state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TicketState {
    /// Waiting for a seat.
    Queued,
    /// A seat is reserved; connect with the join token.
    Assigned,
    /// The instance accepted the join token.
    Joined,
    /// Left the queue.
    Cancelled,
    /// Timed out (queue too long or join token never used).
    Expired,
}

impl TicketState {
    /// Database string.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Assigned => "assigned",
            Self::Joined => "joined",
            Self::Cancelled => "cancelled",
            Self::Expired => "expired",
        }
    }

    /// Parse the database string.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "queued" => Some(Self::Queued),
            "assigned" => Some(Self::Assigned),
            "joined" => Some(Self::Joined),
            "cancelled" => Some(Self::Cancelled),
            "expired" => Some(Self::Expired),
            _ => None,
        }
    }
}

/// Where to connect once assigned.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Assignment {
    /// Instance.
    pub instance_id: InstanceId,
    /// Connect address.
    pub address: String,
    /// One-time token to present to the instance (expires after `join_expires_ms`). A new one
    /// is minted on every status poll; older ones stop working.
    pub join_token: String,
    /// Join token expiry (Unix ms).
    pub join_expires_ms: i64,
    /// Role reserved.
    pub role: Role,
}

/// `GET /v1/matchmaking/status`, also returned by queue and reconnect.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TicketStatus {
    /// Ticket id.
    pub ticket_id: u32,
    /// State.
    pub state: TicketState,
    /// Party the ticket belongs to.
    pub party_id: Option<u32>,
    /// Present when `state == assigned`.
    pub assignment: Option<Assignment>,
}

/// `POST /v1/matchmaking/reconnect`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReconnectRequest {
    /// Token the instance handed out at join.
    pub reconnect_token: String,
}

/// `POST /v1/instances/{id}/join/verify`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifyJoinRequest {
    /// Join token the client presented.
    pub join_token: String,
}

/// Who just joined.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifyJoinResponse {
    /// Durable account id (economy `PlayerId`).
    pub account_id: AccountId,
    /// Public name.
    pub display_name: String,
    /// Role reserved for the player.
    pub role: Role,
    /// Party, if any (spawn party members together).
    pub party_id: Option<u32>,
    /// Hand this to the client; it lets the player rejoin after a disconnect.
    pub reconnect_token: String,
    /// Reconnect token expiry (refreshed while the player appears in heartbeats).
    pub reconnect_expires_ms: i64,
}

// ---------------------------------------------------------------- parties & crews

/// `POST /v1/parties` response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartyCreated {
    /// Party id.
    pub party_id: u32,
    /// Invite code friends use to join.
    pub code: String,
}

/// `POST /v1/parties/{id}/join`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JoinPartyRequest {
    /// Invite code.
    pub code: String,
}

/// `GET /v1/parties/{id}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartyInfo {
    /// Party id.
    pub party_id: u32,
    /// Leader (queues the party).
    pub leader_id: AccountId,
    /// Members including the leader.
    pub members: Vec<AccountId>,
}

/// `POST /v1/crews`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateCrewRequest {
    /// Unique name.
    pub name: String,
    /// Unique 2–5 char tag.
    pub tag: String,
}

/// `GET /v1/crews/{id}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrewInfo {
    /// Crew id.
    pub crew_id: u32,
    /// Name.
    pub name: String,
    /// Tag.
    pub tag: String,
    /// Leader.
    pub leader_id: AccountId,
    /// Members.
    pub members: Vec<AccountId>,
    /// Districts held, as `city/district`.
    pub territory: Vec<String>,
}

/// `POST /v1/instances/{id}/territory`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerritoryReport {
    /// District id within the instance's city.
    pub district_id: String,
    /// New holder; `None` = neutral.
    pub crew_id: Option<u32>,
}

// ---------------------------------------------------------------- results

/// One racer's result in a race report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RaceEntry {
    /// Racer.
    pub account_id: AccountId,
    /// Finishing position, 1 = win, 0 = DNF.
    pub position: u32,
    /// Finish time; `None` for DNF.
    pub finish_time_ms: Option<u64>,
    /// No contact and no violations beyond speeding (same rule as the economy's clean bonus).
    pub clean: bool,
}

/// `POST /v1/instances/{id}/results/race`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RaceReport {
    /// Idempotency key (e.g. `<instance>-<race seq>`); a repeated key is accepted and ignored.
    pub report_key: String,
    /// Route id; time-attack boards are per route.
    pub route_id: String,
    /// Race type id (`wp_economy::RaceType` name); `time_attack` feeds the time boards only.
    pub race_type: String,
    /// Damage mode.
    pub mode: DamageMode,
    /// Class bracket.
    pub bracket: String,
    /// Results.
    pub entries: Vec<RaceEntry>,
}

/// Rating change of one racer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RatingChange {
    /// Racer.
    pub account_id: AccountId,
    /// Rating before.
    pub before: f64,
    /// Rating after.
    pub after: f64,
}

/// Race report result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RaceReportResponse {
    /// Archive id.
    pub race_id: u32,
    /// `true` when this `report_key` had already been recorded (nothing changed).
    pub duplicate: bool,
    /// Clean-race rating changes.
    pub ratings: Vec<RatingChange>,
}

/// How a pursuit ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PursuitOutcome {
    /// The racer got away.
    Escaped,
    /// The racer was captured.
    Captured,
}

/// `POST /v1/instances/{id}/results/pursuit`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PursuitReport {
    /// Idempotency key.
    pub report_key: String,
    /// The pursued racer.
    pub racer_id: AccountId,
    /// Highest wanted level reached.
    pub max_wanted: WantedLevel,
    /// Racer bounty at the end of the pursuit.
    pub bounty: u32,
    /// Outcome.
    pub outcome: PursuitOutcome,
    /// Player cop credited with the capture (AI captures: `None`).
    pub captured_by: Option<AccountId>,
    /// Player cops credited with an assist.
    #[serde(default)]
    pub assists: Vec<AccountId>,
}

/// Pursuit report result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PursuitReportResponse {
    /// Archive id.
    pub pursuit_id: u32,
    /// `true` when this `report_key` had already been recorded.
    pub duplicate: bool,
}

// ---------------------------------------------------------------- economy persistence

/// `POST /v1/instances/{id}/accounts/{account}/economy/commit`: the HTTP form of
/// `AccountStore::commit`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitRequest {
    /// Account after the operation.
    pub account: wp_economy::PlayerAccount,
    /// Events the operation posted (must continue the stored sequence).
    pub events: Vec<wp_economy::EconomyEvent>,
}

// ---------------------------------------------------------------- leaderboards

/// One leaderboard row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LeaderboardRow {
    /// 1-based rank.
    pub rank: u32,
    /// Account.
    pub account_id: AccountId,
    /// Public name.
    pub display_name: String,
    /// Score (rating, bounty, captures, or time in ms for time-attack boards).
    pub score: f64,
}

/// `GET /v1/leaderboards/{board}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Leaderboard {
    /// Board id (`clean_rating`, `bounty`, `captures`, `time_attack:<route>`).
    pub board: String,
    /// Period key (`all`, `d:YYYY-MM-DD`, `w:YYYY-MM-DD`).
    pub period: String,
    /// `true` if lower scores rank higher (time attack).
    pub ascending: bool,
    /// Rows, best first.
    pub rows: Vec<LeaderboardRow>,
}

// ---------------------------------------------------------------- admin

/// `POST /v1/admin/accounts/{id}/ban`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BanRequest {
    /// Shown to the player and recorded in the audit log.
    pub reason: String,
    /// Ban length in seconds; `None` = permanent.
    #[serde(default)]
    pub duration_s: Option<i64>,
}

/// `POST /v1/admin/accounts/{id}/kick`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KickRequest {
    /// Shown to the player.
    pub reason: String,
}

/// `POST /v1/admin/accounts/{id}/economy/adjust`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdjustRequest {
    /// Cash delta (negative debits; a debit may not create debt).
    pub cash: i64,
    /// Required note, recorded on the economy event and the audit log.
    pub note: String,
}

/// `GET /v1/admin/accounts/{id}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AdminAccountView {
    /// Profile.
    pub profile: Profile,
    /// Ban end (Unix ms), if banned.
    pub banned_until_ms: Option<i64>,
    /// Ban reason.
    pub ban_reason: Option<String>,
    /// Economy account (cash, garage, bounty, cop rank...).
    pub economy: Option<wp_economy::PlayerAccount>,
}

/// `PUT /v1/admin/flags/{key}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetFlagRequest {
    /// Any JSON value.
    pub value: serde_json::Value,
}

/// `POST /v1/admin/leaderboards/reset`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResetBoardRequest {
    /// Board id.
    pub board: String,
    /// `daily`, `weekly` or `all` (current period of that kind).
    pub period: String,
}

/// One audit log row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuditEntry {
    /// Row id.
    pub id: u32,
    /// Time (Unix ms).
    pub at_ms: i64,
    /// Who (from `X-Admin-Actor`, default `admin`).
    pub actor: String,
    /// What, e.g. `ban`.
    pub action: String,
    /// Target, e.g. `account:12`.
    pub target: String,
    /// JSON details.
    pub details: serde_json::Value,
}

/// `GET /v1/admin/health`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HealthReport {
    /// Server time (Unix ms).
    pub now_ms: i64,
    /// Schema version.
    pub schema_version: i64,
    /// Registered accounts.
    pub accounts: u32,
    /// Tickets waiting.
    pub queued_tickets: u32,
    /// Every instance not deregistered more than a day ago.
    pub instances: Vec<InstanceInfo>,
}

/// Database string for a role.
pub fn role_str(role: Role) -> &'static str {
    match role {
        Role::Racer => "racer",
        Role::PoliceChase => "police_chase",
        Role::PoliceCaptain => "police_captain",
        Role::Spectator => "spectator",
    }
}

/// Parse [`role_str`].
pub fn parse_role(s: &str) -> Option<Role> {
    match s {
        "racer" => Some(Role::Racer),
        "police_chase" => Some(Role::PoliceChase),
        "police_captain" => Some(Role::PoliceCaptain),
        "spectator" => Some(Role::Spectator),
        _ => None,
    }
}

/// Database string for a damage mode.
pub fn mode_str(mode: DamageMode) -> &'static str {
    match mode {
        DamageMode::Arcade => "arcade",
        DamageMode::Simulation => "simulation",
    }
}

/// Parse [`mode_str`].
pub fn parse_mode(s: &str) -> Option<DamageMode> {
    match s {
        "arcade" => Some(DamageMode::Arcade),
        "simulation" => Some(DamageMode::Simulation),
        _ => None,
    }
}
