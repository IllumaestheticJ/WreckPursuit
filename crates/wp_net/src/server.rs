//! The authoritative server for one city instance, sans-IO.
//!
//! The host (native websocket loop, in-process tests) calls [`ServerInstance::connect`],
//! [`ServerInstance::receive`] and [`ServerInstance::disconnect`] as transport events happen,
//! [`ServerInstance::net_tick`] at 60 Hz, and sends whatever
//! [`ServerInstance::drain_outgoing`] returns. The instance never reads a clock: its time base
//! is the simulation tick.
//!
//! Per net tick: 4 physics ticks (inputs applied per tick from each player's buffer), then for
//! every connected client: damage events for vehicles it tracks, staggered lattice hashes,
//! requested lattice snapshots, typed game events (police / race / economy / weather), interest
//! enter/leave, a 2 Hz captain view for the captain, weather when it changes, and one
//! unreliable snapshot filled by priority up to the client's byte budget (with nearby
//! kinematic traffic at 10 Hz).
//!
//! **Meta join tokens (MC-6):** with [`ServerConfig::require_join_token`] a new player's
//! `Hello` is parked until the host verifies its `join_token` (e.g. with `wp_meta`'s
//! `VerifyJoin`): [`ServerInstance::take_join_requests`], then
//! [`ServerInstance::approve_join`] or [`ServerInstance::reject_join`].

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use wp_contracts::{
    AccountId, AssistProfile, CaptainCommand, CaptainError, EconomyReason, EntityId, PlayerId,
    PoliceEvent, PursuitOutcome, RaceCommand, RaceType, Role, SimEvent, TrafficAgentSnapshot,
    UnitId, UnitOrder, VehicleInput, VehicleState, WantedLevel, WeatherSnapshot, CONTRACT_VERSION,
};
use wp_economy::{
    AccountStore, CarLocation, CarUid, DamageSnapshot, EventReason, InMemoryStore, OwnedCar,
    PlayerAccount,
};
use wp_math::Rng;

use crate::interest::{classify, InterestState, Subject, Viewer};
use crate::lattice::{lattice_hash, LatticeSnapshot};
use crate::netsim::NetSim;
use crate::protocol::{
    unit_order_code, ClientMsg, DamageEvent, EntityKind, Incident, MapUnit, ServerMsg,
    PLAYER_UNIT_BASE, PROTOCOL_VERSION,
};
use crate::quant::{QContact, QTrafficAgent, QVehicle};
use crate::race::{RaceEvent, RaceLobby};
use crate::roles::{RoleConfig, RoleEvent, RoleManager};
use crate::snapshot::{DeltaVehicle, OwnState};
use crate::wire::ByteWriter;
use crate::{Channel, DOWNLINK_BUDGET_BPS, TICKS_PER_NET_TICK, TICK_HZ};

/// How long a **wanted or pursued** racer's car is held after its player disconnects before
/// the pursuit ends as `Captured` and the bust is settled (60 s; B05). The one place this
/// number lives. Players who are not wanted keep [`ServerConfig::reconnect_window_ticks`].
pub const WANTED_DISCONNECT_GRACE_TICKS: u64 = 60 * TICK_HZ;

/// Notice sent when a wanted racer asks for another role (B05).
pub const WANTED_ROLE_SWITCH_REFUSED: &str =
    "you are wanted: lose the police or get caught before switching role";

/// Net ticks between bit-exact own-vehicle snapshots (12 → 5 Hz).
pub const OWN_FULL_EVERY_NET_TICKS: u64 = 12;

/// Input of a disconnected player's car while it is moving: full brake.
const HOLD_BRAKE: VehicleInput = VehicleInput {
    steer: 0.0,
    throttle: 0.0,
    brake: 1.0,
    handbrake: 0.0,
    clutch: 0.0,
    gear_up: false,
    gear_down: false,
    horn: false,
    headlights: false,
    siren: false,
    tool_use: 0,
};

/// Input of a disconnected player's car at a standstill: light pedal + handbrake.
const HOLD_STILL: VehicleInput = VehicleInput {
    steer: 0.0,
    throttle: 0.0,
    brake: 0.4,
    handbrake: 1.0,
    clutch: 0.0,
    gear_up: false,
    gear_down: false,
    horn: false,
    headlights: false,
    siren: false,
    tool_use: 0,
};

/// Transport-level connection handle chosen by the instance.
pub type ConnId = u32;

/// Instance configuration.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// Maximum humans (default 32).
    pub max_players: usize,
    /// Role policy.
    pub roles: RoleConfig,
    /// Car racers drive.
    pub racer_car: String,
    /// Car player police units drive.
    pub police_car: String,
    /// How long a disconnected player's car stays in the world, ticks (default 30 s).
    pub reconnect_window_ticks: u64,
    /// Grace for a disconnected racer who is wanted or pursued, ticks (default
    /// [`WANTED_DISCONNECT_GRACE_TICKS`]): the car is held (braking to a stop, heat and
    /// pursuit frozen); a reconnect resumes it, expiry counts as a capture.
    pub wanted_grace_ticks: u64,
    /// Down-link budget per client, bytes/s (default 40 000).
    pub downlink_budget_bps: usize,
    /// Interval between lattice hashes per vehicle, ticks (default 2 s).
    pub lattice_hash_interval_ticks: u64,
    /// Spawn points `(pos, yaw)`; player `n` uses point `n % len`.
    pub spawn_points: Vec<([f32; 3], f32)>,
    /// Seed for reconnect tokens.
    pub seed: u64,
    /// Furthest ahead of the server an input tick may be, ticks (default 1 s).
    pub max_input_lead_ticks: u64,
    /// New input ticks accepted per second of server time (default 300; nominal 240).
    pub max_inputs_per_second: u32,
    /// Lattice snapshot requests accepted per second per client.
    pub lattice_requests_per_second: u32,
    /// Captain commands accepted per second.
    pub captain_commands_per_second: u32,
    /// New players must present a meta join token, verified by the host (MC-6; default off).
    pub require_join_token: bool,
    /// Kinematic traffic within this distance of a client's car is replicated, m (default 120).
    pub traffic_radius_m: f32,
    /// Most kinematic traffic agents per client snapshot, nearest first (default 48).
    pub max_traffic_agents: usize,
    /// Net ticks between traffic updates (default 6 → 10 Hz; clients extrapolate between).
    pub traffic_every_net_ticks: u64,
    /// Shortest interval between weather snapshots while the regime is unchanged, ticks
    /// (default 5 s; a regime change is sent at the next check, within 1 s).
    pub weather_interval_ticks: u64,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            max_players: 32,
            roles: RoleConfig::default(),
            racer_car: "debug_sedan".into(),
            police_car: "debug_coupe".into(),
            reconnect_window_ticks: 30 * TICK_HZ,
            wanted_grace_ticks: WANTED_DISCONNECT_GRACE_TICKS,
            downlink_budget_bps: DOWNLINK_BUDGET_BPS,
            lattice_hash_interval_ticks: 2 * TICK_HZ,
            spawn_points: grid_spawn_points(64, 8.0),
            seed: 1,
            max_input_lead_ticks: TICK_HZ,
            max_inputs_per_second: 300,
            lattice_requests_per_second: 4,
            captain_commands_per_second: 10,
            require_join_token: false,
            traffic_radius_m: 120.0,
            max_traffic_agents: 48,
            traffic_every_net_ticks: 6,
            weather_interval_ticks: 5 * TICK_HZ,
        }
    }
}

/// `n` spawn points on an 8-wide grid with `spacing` m between cars, all facing +Y.
pub fn grid_spawn_points(n: usize, spacing: f32) -> Vec<([f32; 3], f32)> {
    (0..n)
        .map(|i| {
            let col = (i % 8) as f32 - 3.5;
            let row = (i / 8) as f32;
            ([col * spacing, row * spacing * 1.5, 0.0], 0.0)
        })
        .collect()
}

/// One frame to send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outgoing {
    /// Destination connection.
    pub conn: ConnId,
    /// Channel.
    pub channel: Channel,
    /// Encoded [`ServerMsg`].
    pub bytes: Vec<u8>,
    /// Close the connection after sending this.
    pub close: bool,
}

/// Instance-wide counters for the admin endpoint and tests.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ServerStats {
    /// Physics tick.
    pub tick: u64,
    /// Humans (connected or within the reconnect window).
    pub players: usize,
    /// Of which connected.
    pub connected: usize,
    /// Mean down-link over the last second across connected clients, bytes/s.
    pub downlink_bps_avg: f64,
    /// Worst client down-link over the last second, bytes/s.
    pub downlink_bps_max: f64,
    /// Total bytes queued for sending since start.
    pub bytes_sent_total: u64,
    /// Ticks simulated with a repeated input because the real one had not arrived.
    pub inputs_missing: u64,
    /// Inputs rejected by anti-cheat (bad timing / rate).
    pub inputs_rejected: u64,
    /// Undecodable frames received.
    pub decode_errors: u64,
    /// Wanted racers captured because their player left (grace expired or quit, B05).
    pub abandoned_captures: u64,
    /// Busts settled on player accounts (fine / impound), abandoned ones included.
    pub busts_settled: u64,
    /// Role requests refused because the racer was wanted or pursued (B05).
    pub role_switches_refused: u64,
    /// Lattice snapshots sent in response to hash mismatches.
    pub lattice_resyncs: u64,
    /// Final lattice snapshots sent after a provisional one (model settled since).
    pub lattice_followups: u64,
    /// Typed game events forwarded (counted once per client that received them).
    pub events_sent: u64,
    /// Captain commands accepted by `wp_police` / the net layer.
    pub captain_commands_ok: u64,
    /// Captain commands refused (`CaptainReply` with an error).
    pub captain_commands_refused: u64,
    /// Bytes sent to players since start, by message tag (first byte of the encoding; see
    /// the protocol table in the README).
    pub bytes_by_tag: BTreeMap<u8, u64>,
}

#[derive(Debug, Default)]
struct View {
    interest: InterestState,
    /// Vehicles whose damage this client replicates (interest set + own vehicle).
    tracked: BTreeSet<EntityId>,
    sent: VecDeque<(u64, Vec<(EntityId, QVehicle)>)>,
    acked: BTreeMap<EntityId, (u64, QVehicle)>,
    last_ack: u64,
    lattice_requests: BTreeSet<EntityId>,
    captain_known: BTreeMap<u32, MapUnit>,
    captain_synced: bool,
    bytes_this_tick: usize,
    bytes_window: VecDeque<usize>,
    /// Down-link byte credit (token bucket): refilled per net tick, spent by everything sent,
    /// so a large frame (own-vehicle snapshot) is repaid by the following ticks.
    credit: i64,
    /// Lattice snapshots owed to this client (entities entering interest, resync requests),
    /// sent oldest first as the byte credit allows.
    lattice_queue: VecDeque<EntityId>,
    /// Vehicles whose last lattice snapshot to this client was provisional (the model was
    /// hit and had not settled): re-sent once it has.
    unsettled: BTreeSet<EntityId>,
    /// Encoded size of the queued lattice snapshot the credit is being saved for (0 = none).
    lattice_wait: usize,
}

#[derive(Debug, Clone, Copy, Default)]
struct RateWindow {
    start_tick: u64,
    count: u32,
}

impl RateWindow {
    /// Count one event at `tick`; false if `limit` per second is exceeded.
    fn allow(&mut self, tick: u64, limit: u32) -> bool {
        if tick >= self.start_tick + TICK_HZ {
            self.start_tick = tick;
            self.count = 0;
        }
        self.count += 1;
        self.count <= limit
    }
}

/// A new player's `Hello` waiting for the host to verify its meta join token (MC-6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JoinRequest {
    /// Connection that sent the `Hello`.
    pub conn: ConnId,
    /// Requested display name.
    pub name: String,
    /// The `join_token` presented (may be empty: the host decides).
    pub join_token: String,
}

/// One committed money change of an externally stored account ([`ServerInstance::take_economy_commits`]).
#[derive(Debug, Clone, PartialEq)]
pub struct EconomyCommit {
    /// Instance player.
    pub player: PlayerId,
    /// The account after the change (`account.player` = durable account id).
    pub account: PlayerAccount,
    /// The ledger events of the change (continue the account's sequence).
    pub events: Vec<wp_economy::EconomyEvent>,
}

/// A finished lobby race ([`ServerInstance::take_finished_races`]).
#[derive(Debug, Clone, PartialEq)]
pub struct FinishedRace {
    /// Lobby race id.
    pub race: u32,
    /// Economy race type (one lap = sprint, more = circuit).
    pub race_type: RaceType,
    /// Laps.
    pub laps: u8,
    /// Checkpoints, world XY dm (the route).
    pub checkpoints: Vec<[i32; 2]>,
    /// Results by place (DNF last), payouts filled in.
    pub results: Vec<crate::protocol::RaceResultEntry>,
    /// Racers with no damaging contact and no violation beyond speeding during the race.
    pub clean: Vec<PlayerId>,
}

#[derive(Debug, Clone)]
struct PendingJoin {
    request: JoinRequest,
    assists: AssistProfile,
    handed_out: bool,
}

#[derive(Debug)]
struct Player {
    id: PlayerId,
    name: String,
    token: u64,
    meta_reconnect_token: String,
    conn: Option<ConnId>,
    assists: AssistProfile,
    vehicle: Option<EntityId>,
    car_id: String,
    inputs: BTreeMap<u64, VehicleInput>,
    last_input: VehicleInput,
    ever_sent_input: bool,
    disconnected_at: Option<u64>,
    /// Disconnected while wanted / pursued: the car and its pursuit are held for
    /// [`ServerConfig::wanted_grace_ticks`] and expiry is a capture.
    wanted_hold: bool,
    /// Input for the held car while disconnected (brake to a stop, then hold it there).
    hold_input: VehicleInput,
    view: View,
    input_rate: RateWindow,
    lattice_rate: RateWindow,
    command_rate: RateWindow,
}

/// Contacts and events applied to one vehicle in one physics tick.
#[derive(Debug, Default)]
struct TickDamage {
    contacts: Vec<QContact>,
    events: Vec<DamageEvent>,
}

#[derive(Debug, Clone, Copy)]
struct IncidentRec {
    x: f32,
    y: f32,
    kind: u8,
    tick: u64,
}

/// One authoritative city instance.
pub struct ServerInstance<S: NetSim> {
    cfg: ServerConfig,
    sim: S,
    rng: Rng,
    conns: BTreeMap<ConnId, Option<PlayerId>>,
    next_conn: ConnId,
    players: BTreeMap<PlayerId, Player>,
    next_player: u32,
    roles: RoleManager,
    races: RaceLobby,
    accounts: InMemoryStore,
    /// Owner and kind of every vehicle the net layer spawned.
    owned: BTreeMap<EntityId, (PlayerId, EntityKind)>,
    /// Latest captain order per police vehicle (map markers; player seats follow it).
    unit_orders: BTreeMap<EntityId, UnitOrder>,
    incidents: VecDeque<IncidentRec>,
    pending_damage: Vec<(u64, BTreeMap<EntityId, TickDamage>)>,
    /// Typed events (police / race / economy / weather) since the last client output.
    pending_events: Vec<(u64, SimEvent)>,
    /// Copy of every forwarded game event for the host (`set_event_tap`), until taken.
    tapped_events: Option<Vec<SimEvent>>,
    /// Accounts owned by an external store (meta): instance player → its account (whose
    /// `player` is the durable account id). Money for these players changes here and every
    /// change goes to `economy_outbox` for the host to write through.
    external_accounts: BTreeMap<PlayerId, PlayerAccount>,
    /// Players whose external account is being loaded: credits wait here meanwhile.
    awaiting_account: BTreeMap<PlayerId, Vec<(i64, i64, EconomyReason)>>,
    /// Committed external-account changes not yet taken by the host.
    economy_outbox: Vec<EconomyCommit>,
    /// Finished lobby races not yet taken by the host (only while the event tap is on).
    race_outbox: Vec<FinishedRace>,
    /// Racers in a running race who had a damaging contact or a non-speeding violation.
    race_unclean: std::collections::BTreeSet<PlayerId>,
    /// Hellos parked for join-token verification.
    pending_joins: BTreeMap<ConnId, PendingJoin>,
    /// Kinematic traffic of this net tick (`Some` on traffic ticks).
    traffic_now: Option<Vec<TrafficAgentSnapshot>>,
    /// Last weather broadcast and its tick.
    last_weather: Option<(u64, WeatherSnapshot)>,
    human_captain: bool,
    /// Economy tables for settling busts (loaded on the first one).
    econ: Option<Box<wp_economy::Economy>>,
    outgoing: Vec<Outgoing>,
    net_ticks: u64,
    stats: ServerStats,
}

impl<S: NetSim> ServerInstance<S> {
    /// New instance around a simulation.
    pub fn new(cfg: ServerConfig, sim: S) -> Self {
        Self {
            rng: Rng::new(cfg.seed),
            roles: RoleManager::new(cfg.roles),
            cfg,
            sim,
            conns: BTreeMap::new(),
            next_conn: 1,
            players: BTreeMap::new(),
            next_player: 1,
            races: RaceLobby::new(),
            accounts: InMemoryStore::default(),
            owned: BTreeMap::new(),
            unit_orders: BTreeMap::new(),
            incidents: VecDeque::new(),
            pending_damage: Vec::new(),
            pending_events: Vec::new(),
            tapped_events: None,
            external_accounts: BTreeMap::new(),
            awaiting_account: BTreeMap::new(),
            economy_outbox: Vec::new(),
            race_outbox: Vec::new(),
            race_unclean: Default::default(),
            pending_joins: BTreeMap::new(),
            traffic_now: None,
            last_weather: None,
            human_captain: false,
            econ: None,
            outgoing: Vec::new(),
            net_ticks: 0,
            stats: ServerStats::default(),
        }
    }

    /// The simulation.
    pub fn sim(&self) -> &S {
        &self.sim
    }

    /// The simulation, mutably (tests, admin tools).
    pub fn sim_mut(&mut self) -> &mut S {
        &mut self.sim
    }

    /// Account store (server-side economy; clients never write it).
    pub fn accounts_mut(&mut self) -> &mut InMemoryStore {
        &mut self.accounts
    }

    /// Role manager (read-only).
    pub fn roles(&self) -> &RoleManager {
        &self.roles
    }

    /// Counters.
    pub fn stats(&self) -> ServerStats {
        let mut s = self.stats.clone();
        s.tick = self.sim.tick();
        s.players = self.players.len();
        let rates: Vec<f64> = self
            .players
            .values()
            .filter(|p| p.conn.is_some() && !p.view.bytes_window.is_empty())
            .map(|p| {
                p.view.bytes_window.iter().sum::<usize>() as f64 * 60.0
                    / p.view.bytes_window.len() as f64
            })
            .collect();
        s.connected = self.players.values().filter(|p| p.conn.is_some()).count();
        s.downlink_bps_max = rates.iter().copied().fold(0.0, f64::max);
        s.downlink_bps_avg = if rates.is_empty() {
            0.0
        } else {
            rates.iter().sum::<f64>() / rates.len() as f64
        };
        s
    }

    /// Vehicle driven by a player.
    pub fn vehicle_of(&self, player: PlayerId) -> Option<EntityId> {
        self.players.get(&player).and_then(|p| p.vehicle)
    }

    /// Players in id order with their names.
    pub fn player_names(&self) -> Vec<(PlayerId, String)> {
        self.players
            .values()
            .map(|p| (p.id, p.name.clone()))
            .collect()
    }

    /// Register a new transport connection.
    pub fn connect(&mut self) -> ConnId {
        let c = self.next_conn;
        self.next_conn += 1;
        self.conns.insert(c, None);
        c
    }

    /// Transport closed. A joined player keeps their car for the reconnect window; a racer
    /// who is wanted or pursued gets [`ServerConfig::wanted_grace_ticks`] instead, with the
    /// car held and its heat / pursuit frozen, and expiry counts as a capture (B05).
    pub fn disconnect(&mut self, conn: ConnId) {
        self.pending_joins.remove(&conn);
        if let Some(Some(pid)) = self.conns.remove(&conn) {
            let tick = self.sim.tick();
            let mut hold = None;
            if let Some(p) = self.players.get_mut(&pid) {
                if p.conn == Some(conn) {
                    p.conn = None;
                    p.disconnected_at = Some(tick);
                    p.inputs.clear();
                    p.hold_input = HOLD_BRAKE;
                    hold = p.vehicle;
                }
            }
            if let Some(v) = hold.filter(|v| self.is_wanted_racer(*v)) {
                self.sim.hold_racer(v, true);
                self.players.get_mut(&pid).expect("player").wanted_hold = true;
            }
        }
    }

    /// Whether the player is seated as a racer and their car is wanted or pursued.
    fn wanted_racer_seat(&self, pid: PlayerId) -> bool {
        self.roles.role(pid) == Some(Role::Racer)
            && self
                .players
                .get(&pid)
                .and_then(|p| p.vehicle)
                .is_some_and(|v| self.is_wanted_racer(v))
    }

    /// Whether `v` is a racer seat whose driver is wanted or in a pursuit.
    fn is_wanted_racer(&self, v: EntityId) -> bool {
        self.owned
            .get(&v)
            .is_some_and(|(_, k)| *k == EntityKind::Racer)
            && self.sim.wanted_or_pursued(v)
    }

    /// Frames to send since the last call.
    pub fn drain_outgoing(&mut self) -> Vec<Outgoing> {
        std::mem::take(&mut self.outgoing)
    }

    /// New join requests waiting for token verification (each returned once; see the module
    /// docs). Empty unless [`ServerConfig::require_join_token`] is set.
    pub fn take_join_requests(&mut self) -> Vec<JoinRequest> {
        self.pending_joins
            .values_mut()
            .filter(|p| !p.handed_out)
            .map(|p| {
                p.handed_out = true;
                p.request.clone()
            })
            .collect()
    }

    /// Admit a parked join: the player joins as `display_name` (`None` keeps the requested
    /// name) and `Welcome` carries `meta_reconnect_token`. Returns the new player, or `None`
    /// if the connection is gone or the instance filled up meanwhile (it is then rejected).
    pub fn approve_join(
        &mut self,
        conn: ConnId,
        display_name: Option<String>,
        meta_reconnect_token: String,
    ) -> Option<PlayerId> {
        let pending = self.pending_joins.remove(&conn)?;
        if !self.conns.contains_key(&conn) {
            return None;
        }
        if self.players.len() >= self.cfg.max_players {
            let reason = "instance full".to_string();
            self.send_conn(conn, &ServerMsg::Reject { reason }, true);
            return None;
        }
        let name = display_name.unwrap_or(pending.request.name);
        Some(self.join(conn, name, pending.assists, meta_reconnect_token))
    }

    /// Put a joined player in `role` as if they had asked for it (the host decides, e.g. the
    /// role meta reserved at matchmaking). Policy (police share, one captain) still applies.
    pub fn assign_role(&mut self, pid: PlayerId, role: Role) {
        let tick = self.sim.tick();
        let events = self.roles.request(pid, role, tick);
        self.apply_role_events(events);
    }

    /// Disconnect a player for good (no reconnect window): `Reject { reason }`, then close.
    pub fn kick(&mut self, pid: PlayerId, reason: impl Into<String>) {
        let Some(conn) = self.players.get(&pid).and_then(|p| p.conn) else {
            self.remove_player(pid);
            return;
        };
        let reason = reason.into();
        self.send_conn(conn, &ServerMsg::Reject { reason }, true);
        self.remove_player(pid);
    }

    /// Players with a live connection and their roles, ascending.
    pub fn connected_players(&self) -> Vec<(PlayerId, Role)> {
        self.players
            .values()
            .filter(|p| p.conn.is_some())
            .map(|p| (p.id, self.roles.role(p.id).unwrap_or(Role::Spectator)))
            .collect()
    }

    /// The player driving vehicle `e`, if a player does.
    pub fn owner_of(&self, e: EntityId) -> Option<PlayerId> {
        self.owned.get(&e).map(|(p, _)| *p)
    }

    /// The player driving the police seat embodied by `wp_police` unit `unit` (player cops).
    pub fn player_of_unit(&self, unit: UnitId) -> Option<PlayerId> {
        self.owned
            .iter()
            .find(|(e, (_, k))| {
                *k == EntityKind::PoliceChase && self.sim.police_unit(**e) == Some(unit)
            })
            .map(|(_, (p, _))| *p)
    }

    /// Start (`true`) or stop keeping a copy of every game event the server forwards
    /// (police, race, economy, weather) for the host, e.g. meta pursuit reports.
    pub fn set_event_tap(&mut self, on: bool) {
        self.tapped_events = on.then(Vec::new);
    }

    /// Events collected since the last call (see [`ServerInstance::set_event_tap`]).
    pub fn take_tapped_events(&mut self) -> Vec<SimEvent> {
        self.tapped_events
            .as_mut()
            .map(std::mem::take)
            .unwrap_or_default()
    }

    /// Refuse a parked join: the client gets `Reject { reason }` and the connection closes.
    pub fn reject_join(&mut self, conn: ConnId, reason: impl Into<String>) {
        if self.pending_joins.remove(&conn).is_some() {
            let reason = reason.into();
            self.send_conn(conn, &ServerMsg::Reject { reason }, true);
        }
    }

    fn send_conn(&mut self, conn: ConnId, msg: &ServerMsg, close: bool) {
        let bytes = msg.encode();
        self.stats.bytes_sent_total += bytes.len() as u64;
        self.outgoing.push(Outgoing {
            conn,
            channel: msg.channel(),
            bytes,
            close,
        });
    }

    fn send(&mut self, player: PlayerId, msg: &ServerMsg) {
        let Some(p) = self.players.get_mut(&player) else {
            return;
        };
        let Some(conn) = p.conn else { return };
        let bytes = msg.encode();
        p.view.bytes_this_tick += bytes.len();
        self.stats.bytes_sent_total += bytes.len() as u64;
        if let Some(tag) = bytes.first() {
            *self.stats.bytes_by_tag.entry(*tag).or_default() += bytes.len() as u64;
        }
        self.outgoing.push(Outgoing {
            conn,
            channel: msg.channel(),
            bytes,
            close: false,
        });
    }

    fn notice(&mut self, player: PlayerId, text: impl Into<String>) {
        self.send(player, &ServerMsg::Notice { text: text.into() });
    }

    fn broadcast(&mut self, msg: &ServerMsg) {
        let ids: Vec<PlayerId> = self.players.keys().copied().collect();
        for p in ids {
            self.send(p, msg);
        }
    }

    // -----------------------------------------------------------------------------------------
    // Receiving
    // -----------------------------------------------------------------------------------------

    /// Handle one received frame.
    pub fn receive(&mut self, conn: ConnId, bytes: &[u8]) {
        let Some(slot) = self.conns.get(&conn).copied() else {
            return;
        };
        let msg = match ClientMsg::decode(bytes) {
            Ok(m) => m,
            Err(_) => {
                self.stats.decode_errors += 1;
                return;
            }
        };
        match (slot, msg) {
            (
                None,
                ClientMsg::Hello {
                    protocol,
                    contract,
                    name,
                    input_source,
                    reconnect_token,
                    join_token,
                },
            ) => {
                if protocol != PROTOCOL_VERSION || contract != CONTRACT_VERSION {
                    let reason = format!(
                        "version mismatch: server protocol {PROTOCOL_VERSION} contract {CONTRACT_VERSION}"
                    );
                    self.send_conn(conn, &ServerMsg::Reject { reason }, true);
                    return;
                }
                let assists = AssistProfile::for_source(input_source);
                if reconnect_token != 0 {
                    if let Some(pid) = self
                        .players
                        .values()
                        .find(|p| p.token == reconnect_token && p.conn.is_none())
                        .map(|p| p.id)
                    {
                        self.reattach(conn, pid);
                        return;
                    }
                }
                if self.players.len() >= self.cfg.max_players {
                    let reason = "instance full".to_string();
                    self.send_conn(conn, &ServerMsg::Reject { reason }, true);
                    return;
                }
                if self.cfg.require_join_token {
                    // The host verifies the token (MC-6); a second Hello replaces the first.
                    self.pending_joins.insert(
                        conn,
                        PendingJoin {
                            request: JoinRequest {
                                conn,
                                name,
                                join_token,
                            },
                            assists,
                            handed_out: false,
                        },
                    );
                    return;
                }
                self.join(conn, name, assists, String::new());
            }
            (None, _) => {} // must say Hello first
            (Some(_), ClientMsg::Hello { .. }) => {}
            (Some(pid), msg) => self.handle(pid, msg),
        }
    }

    fn join(
        &mut self,
        conn: ConnId,
        name: String,
        assists: AssistProfile,
        meta_reconnect_token: String,
    ) -> PlayerId {
        let id = PlayerId(self.next_player);
        self.next_player += 1;
        let token = loop {
            let t = self.rng.next_u64();
            if t != 0 {
                break t;
            }
        };
        let name = if name.trim().is_empty() {
            format!("player{}", id.0)
        } else {
            name
        };
        self.players.insert(
            id,
            Player {
                id,
                name,
                token,
                meta_reconnect_token,
                conn: Some(conn),
                assists,
                vehicle: None,
                car_id: String::new(),
                inputs: BTreeMap::new(),
                last_input: VehicleInput::default(),
                ever_sent_input: false,
                disconnected_at: None,
                wanted_hold: false,
                hold_input: HOLD_BRAKE,
                view: View::default(),
                input_rate: RateWindow::default(),
                lattice_rate: RateWindow::default(),
                command_rate: RateWindow::default(),
            },
        );
        self.conns.insert(conn, Some(id));
        let cop_rank = self.account(id).cop_rank;
        self.roles.join(id, cop_rank);
        self.welcome(id);
        self.send_role(id, Role::Spectator);
        id
    }

    fn reattach(&mut self, conn: ConnId, pid: PlayerId) {
        self.conns.insert(conn, Some(pid));
        let p = self.players.get_mut(&pid).expect("player exists");
        p.conn = Some(conn);
        p.disconnected_at = None;
        p.view = View::default();
        p.inputs.clear();
        p.last_input = VehicleInput::default();
        if std::mem::take(&mut p.wanted_hold) {
            // Back within the grace: the same pursuit and heat resume.
            if let Some(v) = p.vehicle {
                self.sim.hold_racer(v, false);
            }
        }
        let role = self.roles.role(pid).unwrap_or(Role::Spectator);
        self.welcome(pid);
        self.send_role(pid, role);
        if let Some(race) = self.races.race_of(pid) {
            if let Some(info) = self.races.info(race).cloned() {
                self.send(pid, &ServerMsg::RaceUpdate(info));
            }
        }
    }

    fn welcome(&mut self, pid: PlayerId) {
        let p = &self.players[&pid];
        let msg = ServerMsg::Welcome {
            player: pid,
            token: p.token,
            server_tick: self.sim.tick(),
            mode: self.sim.damage_mode(),
            protocol: PROTOCOL_VERSION,
            time_of_day_h: self.sim.time_of_day().hours(),
            meta_reconnect_token: p.meta_reconnect_token.clone(),
        };
        self.send(pid, &msg);
        self.send_account(pid);
        if let Some(w) = self.sim.weather() {
            self.send(pid, &ServerMsg::Weather(w));
        }
    }

    /// The player's account, or an empty one if none is stored yet (lead adapter to
    /// `wp_economy::AccountStore`; replace with `Economy::open_account` + `settle_race`, NET-N7).
    fn account(&self, pid: PlayerId) -> PlayerAccount {
        if let Some(a) = self.external_accounts.get(&pid) {
            return a.clone();
        }
        self.accounts
            .load(pid)
            .ok()
            .flatten()
            .unwrap_or_else(|| PlayerAccount::empty(pid))
    }

    fn send_account(&mut self, pid: PlayerId) {
        let a = self.account(pid);
        self.send(
            pid,
            &ServerMsg::Account {
                cash: a.cash,
                rep: a.rep,
                cop_rank: a.cop_rank,
                bounty: a.bounty,
            },
        );
    }

    fn handle(&mut self, pid: PlayerId, msg: ClientMsg) {
        let tick = self.sim.tick();
        match msg {
            ClientMsg::Hello { .. } => {}
            ClientMsg::Inputs {
                ack_snapshot_tick,
                first_tick,
                inputs,
                missing,
            } => {
                self.ack_snapshot(pid, ack_snapshot_tick);
                // The client lacks these baselines: next update goes out in full.
                let acked = &mut self.players.get_mut(&pid).expect("player").view.acked;
                for e in missing {
                    acked.remove(&EntityId(e));
                }
                self.accept_inputs(pid, first_tick, &ClientMsg::inputs_dequantised(&inputs));
            }
            ClientMsg::RoleRequest { role } => {
                if role != Role::Racer && self.wanted_racer_seat(pid) {
                    // Switching away drops the racer car: while wanted that would be an
                    // escape (B05). Refused like other role refusals: the current role again,
                    // with the reason as a notice.
                    self.send_role(pid, Role::Racer);
                    self.notice(pid, WANTED_ROLE_SWITCH_REFUSED);
                    self.stats.role_switches_refused += 1;
                    return;
                }
                let events = self.roles.request(pid, role, tick);
                self.apply_role_events(events);
            }
            ClientMsg::CaptainCommand { seq, command } => {
                let result = self.captain_command(pid, command);
                if result.is_ok() {
                    self.stats.captain_commands_ok += 1;
                } else {
                    self.stats.captain_commands_refused += 1;
                }
                self.send(pid, &ServerMsg::CaptainReply { seq, result });
            }
            ClientMsg::Ping { client_ms } => {
                self.send(
                    pid,
                    &ServerMsg::Pong {
                        client_ms,
                        server_tick: tick,
                    },
                );
            }
            ClientMsg::LatticeRequest { vehicle } => {
                let limit = self.cfg.lattice_requests_per_second;
                let p = self.players.get_mut(&pid).expect("player");
                let e = EntityId(vehicle);
                if p.view.tracked.contains(&e) && p.lattice_rate.allow(tick, limit) {
                    p.view.lattice_requests.insert(e);
                }
            }
            ClientMsg::RaceCreate { laps, checkpoints } => {
                if self.roles.role(pid) != Some(Role::Racer) {
                    self.notice(pid, "only racers can host races");
                    return;
                }
                let time = self.sim.time_of_day();
                match self.races.create(pid, laps, checkpoints, time) {
                    Ok(ev) => self.apply_race_events(vec![ev]),
                    Err(e) => self.notice(pid, e),
                }
            }
            ClientMsg::RaceInvite { race, player } => {
                let invitee = PlayerId(player);
                if self.races.can_invite(pid, race) && self.players.contains_key(&invitee) {
                    self.send(invitee, &ServerMsg::RaceInvited { race, from: pid.0 });
                } else {
                    self.notice(pid, "cannot invite to that race");
                }
            }
            ClientMsg::RaceCommand(cmd) => self.race_command(pid, cmd, tick),
            ClientMsg::Bye => {
                // Quitting while wanted is a bust, not an escape (B05).
                self.capture_abandoned(pid);
                self.remove_player(pid);
            }
        }
    }

    fn ack_snapshot(&mut self, pid: PlayerId, ack: u64) {
        let p = self.players.get_mut(&pid).expect("player");
        let v = &mut p.view;
        if ack <= v.last_ack {
            return;
        }
        v.last_ack = ack;
        while let Some((t, entities)) = v.sent.front() {
            if *t > ack {
                break;
            }
            if *t == ack {
                for (e, q) in entities {
                    if v.interest.contains(*e) {
                        v.acked.insert(*e, (ack, *q));
                    }
                }
            }
            v.sent.pop_front();
        }
    }

    /// Anti-cheat: inputs were already clamped by quantisation; here the timing is checked.
    /// Ticks already simulated are ignored, ticks more than `max_input_lead_ticks` ahead and
    /// anything beyond `max_inputs_per_second` new ticks are rejected and counted.
    fn accept_inputs(&mut self, pid: PlayerId, first_tick: u64, inputs: &[VehicleInput]) {
        let now = self.sim.tick();
        let lead = self.cfg.max_input_lead_ticks;
        let limit = self.cfg.max_inputs_per_second;
        let p = self.players.get_mut(&pid).expect("player");
        if p.vehicle.is_none() {
            return;
        }
        for (k, input) in inputs.iter().enumerate() {
            let t = first_tick.saturating_add(k as u64);
            if t < now || p.inputs.contains_key(&t) {
                continue;
            }
            if t > now + lead || !p.input_rate.allow(now, limit) {
                self.stats.inputs_rejected += 1;
                continue;
            }
            p.inputs.insert(t, *input);
            p.ever_sent_input = true;
        }
    }

    /// Race lobby commands (contract `RaceCommand`, v0.3.0): `Enter`/`Withdraw` name the
    /// lobby race by its id, `Ready` from the host starts the countdown.
    fn race_command(&mut self, pid: PlayerId, cmd: RaceCommand, tick: u64) {
        match cmd {
            RaceCommand::Enter { event } => {
                if self.roles.role(pid) != Some(Role::Racer) {
                    self.notice(pid, "only racers can join races");
                    return;
                }
                match self.races.join(pid, event.0) {
                    Ok(ev) => self.apply_race_events(vec![ev]),
                    Err(e) => self.notice(pid, e),
                }
            }
            RaceCommand::Withdraw { event } => {
                if let Some(ev) = self.races.leave(pid, event.0) {
                    self.apply_race_events(vec![ev]);
                }
            }
            RaceCommand::Ready => {
                let Some(race) = self.races.race_of(pid) else {
                    self.notice(pid, "not in a race");
                    return;
                };
                let is_host = self.races.info(race).is_some_and(|i| i.host == pid.0);
                if !is_host {
                    // Members are ready by joining; the host's Ready starts the countdown.
                    self.notice(pid, "ready; waiting for the host");
                    return;
                }
                match self.races.start(pid, race, tick) {
                    Ok(ev) => self.apply_race_events(vec![ev]),
                    Err(e) => self.notice(pid, e),
                }
            }
        }
    }

    /// Typed captain command (contract `CaptainCommand`, v0.3.0); player chase units may send
    /// the tool requests (roadblock / spikes / helicopter) too. AI units and tools go to
    /// `wp_police` through the sim; orders for player chase seats (unit ids from
    /// [`PLAYER_UNIT_BASE`]) are relayed to the seat's driver as [`ServerMsg::UnitOrder`].
    fn captain_command(&mut self, pid: PlayerId, cmd: CaptainCommand) -> Result<(), CaptainError> {
        let tick = self.sim.tick();
        // Chase units may call in tools on the radio (roadblock, spikes, helicopter); unit
        // orders, priority and call-off stay with the captain.
        let field_request = matches!(
            cmd,
            CaptainCommand::RequestRoadblock { .. }
                | CaptainCommand::RequestSpikes { .. }
                | CaptainCommand::RequestHelicopter { .. }
        );
        let allowed = match self.roles.role(pid) {
            Some(Role::PoliceCaptain) => true,
            Some(Role::PoliceChase) => field_request,
            _ => false,
        };
        if !allowed {
            self.notice(pid, "only the captain can command units");
            return Err(CaptainError::Unavailable);
        }
        let limit = self.cfg.captain_commands_per_second;
        let rate = &mut self.players.get_mut(&pid).expect("player").command_rate;
        if !rate.allow(tick, limit) {
            let left = (rate.start_tick + TICK_HZ).saturating_sub(tick);
            return Err(CaptainError::Cooldown {
                remaining_s: left as f32 / TICK_HZ as f32,
            });
        }
        if let CaptainCommand::AssignUnit { unit, order } = cmd {
            if unit.0 >= PLAYER_UNIT_BASE {
                return self.order_player_unit(EntityId(unit.0 - PLAYER_UNIT_BASE), order);
            }
            if let Some(e) = self.vehicle_of_unit(unit) {
                if let Some((_, EntityKind::PoliceChase)) = self.owned.get(&e) {
                    // A player seat named by its `wp_police` unit id: same as its wire id.
                    return self.order_player_unit(e, order);
                }
                self.sim.captain_command(cmd)?;
                self.unit_orders.insert(e, order);
                return Ok(());
            }
            return self.sim.captain_command(cmd);
        }
        self.sim.captain_command(cmd)
    }

    fn order_player_unit(&mut self, e: EntityId, order: UnitOrder) -> Result<(), CaptainError> {
        let Some((owner, EntityKind::PoliceChase)) = self.owned.get(&e).copied() else {
            return Err(CaptainError::UnknownUnit);
        };
        if let UnitOrder::Pursue { target } | UnitOrder::Intercept { target } = order {
            if target == e || !self.sim.vehicles().iter().any(|(v, _)| *v == target) {
                return Err(CaptainError::UnknownTarget);
            }
        }
        // Seats bound to a `wp_police` player unit (the real sim): police validate the order
        // and assign the unit, so it counts as "on the racer" for capture credit.
        if let Some(unit) = self.sim.police_unit(e) {
            self.sim
                .captain_command(CaptainCommand::AssignUnit { unit, order })?;
        }
        self.unit_orders.insert(e, order);
        self.send(owner, &ServerMsg::UnitOrder(order));
        Ok(())
    }

    fn vehicle_of_unit(&self, unit: UnitId) -> Option<EntityId> {
        self.sim
            .vehicles()
            .iter()
            .map(|(e, _)| *e)
            .find(|e| self.sim.police_unit(*e) == Some(unit))
    }

    // -----------------------------------------------------------------------------------------
    // Roles, vehicles, races
    // -----------------------------------------------------------------------------------------

    fn send_role(&mut self, pid: PlayerId, role: Role) {
        let (vehicle, car_id) = {
            let p = &self.players[&pid];
            (p.vehicle, p.car_id.clone())
        };
        self.send(
            pid,
            &ServerMsg::RoleAssigned {
                player: pid,
                role,
                vehicle,
                car_id,
            },
        );
        if let Some(v) = vehicle {
            // The client's own damage replica starts from this snapshot.
            self.players
                .get_mut(&pid)
                .expect("player")
                .view
                .tracked
                .insert(v);
            self.send_lattice(pid, v);
        }
    }

    fn apply_role_events(&mut self, events: Vec<RoleEvent>) {
        for ev in events {
            match ev {
                RoleEvent::Assigned {
                    player,
                    role,
                    reason,
                } => {
                    self.set_vehicle_for_role(player, role);
                    self.send_role(player, role);
                    if let Some(r) = reason {
                        self.notice(player, r);
                    }
                }
                RoleEvent::Offer {
                    player,
                    expires_tick,
                } => self.send(player, &ServerMsg::CaptainOffer { expires_tick }),
            }
        }
        // Without a human captain, `wp_police`'s dispatcher places tools itself.
        let seated = self.roles.captain().is_some();
        if seated != self.human_captain {
            self.human_captain = seated;
            self.sim.set_human_captain(seated);
        }
    }

    fn set_vehicle_for_role(&mut self, pid: PlayerId, role: Role) {
        let wanted: Option<(String, EntityKind)> = match role {
            Role::Racer => Some((self.cfg.racer_car.clone(), EntityKind::Racer)),
            Role::PoliceChase => Some((self.cfg.police_car.clone(), EntityKind::PoliceChase)),
            Role::PoliceCaptain | Role::Spectator => None,
        };
        let (current, current_car) = {
            let p = &self.players[&pid];
            (p.vehicle, p.car_id.clone())
        };
        if let (Some(v), Some((car, kind))) = (current, &wanted) {
            if *car == current_car {
                self.sim.set_owned_kind(v, *kind, pid);
                self.owned.insert(v, (pid, *kind));
                return;
            }
        }
        if let Some(v) = current {
            self.release_vehicle(v);
            let view = &mut self.players.get_mut(&pid).expect("player").view;
            view.tracked.remove(&v);
        }
        if role != Role::Racer {
            if let Some(race) = self.races.race_of(pid) {
                if let Some(ev) = self.races.leave(pid, race) {
                    self.apply_race_events(vec![ev]);
                }
            }
        }
        let mut new_vehicle = None;
        let mut new_car = String::new();
        if let Some((car, kind)) = wanted {
            let (pos, yaw) = self.cfg.spawn_points[pid.0 as usize % self.cfg.spawn_points.len()];
            match self.sim.spawn(&car, pos, yaw) {
                Ok(v) => {
                    let assists = self.players[&pid].assists;
                    self.sim.set_assists(v, assists);
                    self.sim.set_owned_kind(v, kind, pid);
                    self.owned.insert(v, (pid, kind));
                    new_vehicle = Some(v);
                    new_car = car;
                }
                Err(e) => self.notice(pid, format!("spawn failed: {e}")),
            }
        }
        let p = self.players.get_mut(&pid).expect("player");
        p.vehicle = new_vehicle;
        p.car_id = new_car;
        p.inputs.clear();
        p.last_input = VehicleInput::default();
    }

    fn release_vehicle(&mut self, v: EntityId) {
        self.sim.despawn(v);
        self.owned.remove(&v);
        self.unit_orders.remove(&v);
    }

    fn remove_player(&mut self, pid: PlayerId) {
        let tick = self.sim.tick();
        if let Some(race) = self.races.race_of(pid) {
            if let Some(ev) = self.races.leave(pid, race) {
                self.apply_race_events(vec![ev]);
            }
        }
        let Some(p) = self.players.remove(&pid) else {
            return;
        };
        self.external_accounts.remove(&pid);
        self.awaiting_account.remove(&pid);
        self.race_unclean.remove(&pid);
        if let Some(v) = p.vehicle {
            self.release_vehicle(v);
        }
        if let Some(c) = p.conn {
            self.conns.insert(c, None);
        }
        let events = self.roles.leave(pid, tick);
        self.apply_role_events(events);
    }

    fn apply_race_events(&mut self, events: Vec<RaceEvent>) {
        for ev in events {
            match ev {
                RaceEvent::Updated(info) => self.broadcast(&ServerMsg::RaceUpdate(info)),
                RaceEvent::Progress {
                    race,
                    player,
                    next_checkpoint,
                } => {
                    let members = self.races.info(race).map(|i| i.members.clone());
                    let msg = ServerMsg::RaceProgress {
                        race,
                        player: player.0,
                        next_checkpoint,
                    };
                    for m in members.unwrap_or_default() {
                        self.send(PlayerId(m), &msg);
                    }
                }
                RaceEvent::StakesCharged { race, charges } => {
                    // The race starts: everyone starts clean.
                    for (p, _) in &charges {
                        self.race_unclean.remove(p);
                    }
                    let race = self.race_type(race);
                    for (p, amount) in charges {
                        self.credit(p, -amount, 0, EconomyReason::StakeCharged { race });
                    }
                }
                RaceEvent::Finished { race, results } => {
                    let kind = self.race_type(race);
                    for r in &results {
                        let p = PlayerId(r.player);
                        if r.payout != 0 {
                            let reason = EconomyReason::RacePayout {
                                race: kind,
                                placement: r.place,
                            };
                            self.credit(p, r.payout, 0, reason);
                        }
                        if r.place == 1 {
                            let reason = EconomyReason::RaceRep { placement: r.place };
                            self.credit(p, 0, 10, reason);
                        }
                    }
                    if self.tapped_events.is_some() {
                        let info = self.races.info(race);
                        self.race_outbox.push(FinishedRace {
                            race,
                            race_type: kind,
                            laps: info.map_or(1, |i| i.laps),
                            checkpoints: info.map(|i| i.checkpoints.clone()).unwrap_or_default(),
                            clean: results
                                .iter()
                                .map(|r| PlayerId(r.player))
                                .filter(|p| !self.race_unclean.contains(p))
                                .collect(),
                            results: results.clone(),
                        });
                    }
                    for r in &results {
                        self.race_unclean.remove(&PlayerId(r.player));
                    }
                    let msg = ServerMsg::RaceResults {
                        race,
                        entries: results.clone(),
                    };
                    for r in &results {
                        self.send(PlayerId(r.player), &msg);
                    }
                }
            }
        }
    }

    /// Economy race type of a lobby race: one lap is a sprint, more laps a circuit.
    fn race_type(&self, race: u32) -> RaceType {
        match self.races.info(race) {
            Some(i) if i.laps > 1 => RaceType::Circuit,
            _ => RaceType::Sprint,
        }
    }

    /// Server-side economy: the only place money changes. Each change is one ledger event
    /// (`StakeCharged` / `RacePayout` / `RaceRep`, NET-4) committed with the account and
    /// sent to its owner as a typed `SimEvent::Economy`. Until `wp_economy`'s settlement
    /// takes over (NET-N7) the amounts come from the lobby's stake table.
    fn credit(&mut self, pid: PlayerId, cash: i64, rep: i64, reason: EconomyReason) {
        if let Some(waiting) = self.awaiting_account.get_mut(&pid) {
            // The external account is still loading: apply once it is in.
            waiting.push((cash, rep, reason));
            return;
        }
        let external = self.external_accounts.contains_key(&pid);
        let mut a = self.account(pid);
        let rep_before = a.rep;
        a.cash += cash;
        a.rep = (a.rep + rep).max(0);
        let event = wp_economy::EconomyEvent {
            player: a.player,
            seq: a.next_event_seq,
            day: a.day,
            reason: EventReason::from_contract(&reason),
            car: None,
            cash_delta: cash,
            rep_delta: a.rep - rep_before,
            bounty_delta: 0,
            cop_xp_delta: 0,
            cash_after: a.cash,
        };
        a.next_event_seq += 1;
        let committed = if external {
            // The host writes it through to the external store (meta) from the outbox.
            self.external_accounts.insert(pid, a.clone());
            self.economy_outbox.push(EconomyCommit {
                player: pid,
                account: a,
                events: vec![event.clone()],
            });
            true
        } else {
            self.accounts
                .commit(&a, std::slice::from_ref(&event))
                .is_ok()
        };
        if committed {
            let tick = self.sim.tick();
            self.pending_events
                .push((tick, SimEvent::Economy(event.to_contract())));
        }
        self.send_account(pid);
    }

    /// End a wanted racer's pursuit as `Captured` because its player left (grace expired or
    /// `Bye`) and settle the bust. No-op for players who are not wanted racers.
    fn capture_abandoned(&mut self, pid: PlayerId) {
        let Some(v) = self.players.get(&pid).and_then(|p| p.vehicle) else {
            return;
        };
        if !self.is_wanted_racer(v) {
            return;
        }
        if let Some((level, _capturer)) = self.sim.capture_abandoned(v) {
            self.stats.abandoned_captures += 1;
            self.settle_capture(pid, level, false);
        }
    }

    /// Settle a bust on the player's account like any capture (`wp_economy::handle_capture`:
    /// fine, bail, rep loss, car impounded / seized / destroyed by mode). Each ledger event is
    /// committed and sent to the owner as a typed `SimEvent::Economy`.
    ///
    /// Server accounts do not own cars yet (everyone drives `racer_car`), so the car being
    /// driven is registered in the account's garage first if it is not there.
    fn settle_capture(&mut self, pid: PlayerId, level: WantedLevel, total_loss: bool) {
        let tick = self.sim.tick();
        let car_id = self
            .players
            .get(&pid)
            .map(|p| p.car_id.clone())
            .unwrap_or_default();
        let spec = self.sim.car_spec(&car_id);
        let mode = self.sim.damage_mode();
        let mut a = self.account(pid);
        let econ = self
            .econ
            .get_or_insert_with(|| Box::new(wp_economy::Economy::embedded()));
        let car = match a
            .garage
            .iter()
            .find(|c| c.def_id == car_id && c.location == CarLocation::Garage)
        {
            Some(c) => c.uid,
            None => {
                let f = &econ.market.floor_car;
                let spec = spec.unwrap_or_else(|| wp_economy::CarSpec {
                    def_id: car_id.clone(),
                    class: f.class,
                    base_price: f.base_price,
                });
                let uid = CarUid(a.next_car_uid);
                a.next_car_uid += 1;
                a.garage.push(OwnedCar {
                    uid,
                    def_id: spec.def_id,
                    class: spec.class,
                    base_price: spec.base_price,
                    mods: Vec::new(),
                    damage: DamageSnapshot::pristine(),
                    location: CarLocation::Garage,
                    insurance: None,
                    // A stand-in car for the capture settlement: valued like an old save.
                    purchase_price: None,
                    tires: Default::default(),
                });
                uid
            }
        };
        let ctx = wp_economy::CaptureContext {
            mode,
            wanted: level,
            car,
            total_loss,
            session_violations: Default::default(),
        };
        // Own stream per bust so reconnect tokens stay independent of captures.
        let mut rng = Rng::new(self.cfg.seed ^ tick.rotate_left(17) ^ u64::from(pid.0));
        match econ.handle_capture(&mut a, &ctx, &mut rng) {
            Ok((_outcome, events)) => {
                if self.accounts.commit(&a, &events).is_ok() {
                    self.stats.busts_settled += 1;
                    for ev in &events {
                        self.pending_events
                            .push((tick, SimEvent::Economy(ev.to_contract())));
                    }
                }
            }
            Err(e) => {
                // Should not happen (the car was just made sure of); keep the account as is.
                let _ = e;
            }
        }
        if self.players.contains_key(&pid) {
            self.send_account(pid);
        }
    }

    /// Held cars of disconnected players: brake to a stop, then hold it with a light pedal
    /// and the handbrake (a full pedal at a standstill engages reverse in `wp_vehicle`'s
    /// automatic gearbox).
    fn update_hold_inputs(&mut self) {
        if self.players.values().all(|p| p.conn.is_some()) {
            return;
        }
        let states: BTreeMap<EntityId, VehicleState> = self.sim.vehicles().into_iter().collect();
        for p in self.players.values_mut().filter(|p| p.conn.is_none()) {
            let Some(s) = p.vehicle.and_then(|v| states.get(&v)) else {
                continue;
            };
            let speed = (s.lin_vel[0] * s.lin_vel[0] + s.lin_vel[1] * s.lin_vel[1]).sqrt();
            p.hold_input = if speed > 1.0 { HOLD_BRAKE } else { HOLD_STILL };
        }
    }

    /// Change a player's money and rep as one ledger event (`reason`), exactly like the lobby
    /// does; e.g. a host re-applying changes after reloading an external account.
    pub fn credit_player(&mut self, pid: PlayerId, cash: i64, rep: i64, reason: EconomyReason) {
        self.credit(pid, cash, rep, reason);
    }

    /// The player's account lives in an external store (meta): money changes for it wait
    /// until [`ServerInstance::set_external_account`] provides the stored account.
    pub fn expect_external_account(&mut self, pid: PlayerId) {
        if !self.external_accounts.contains_key(&pid) {
            self.awaiting_account.entry(pid).or_default();
        }
    }

    /// Install (or replace, after a reload) the player's externally stored account. From now
    /// on every money change of this player is a commit in
    /// [`ServerInstance::take_economy_commits`]; changes made while it was loading are applied
    /// now. Economy events for this player carry the account's durable id.
    pub fn set_external_account(&mut self, pid: PlayerId, account: PlayerAccount) {
        if !self.players.contains_key(&pid) {
            return;
        }
        self.external_accounts.insert(pid, account);
        for (cash, rep, reason) in self.awaiting_account.remove(&pid).unwrap_or_default() {
            self.credit(pid, cash, rep, reason);
        }
        self.send_account(pid);
    }

    /// The player's account as the server holds it now.
    pub fn player_account(&self, pid: PlayerId) -> PlayerAccount {
        self.account(pid)
    }

    /// Money changes of externally stored accounts since the last call, in commit order.
    pub fn take_economy_commits(&mut self) -> Vec<EconomyCommit> {
        std::mem::take(&mut self.economy_outbox)
    }

    /// Lobby races that finished since the last call (collected while the event tap is on,
    /// see [`ServerInstance::set_event_tap`]).
    pub fn take_finished_races(&mut self) -> Vec<FinishedRace> {
        std::mem::take(&mut self.race_outbox)
    }

    /// Durable account key of a player for economy events (external account id, else the
    /// instance id).
    fn economy_key(&self, pid: PlayerId) -> AccountId {
        AccountId(
            self.external_accounts
                .get(&pid)
                .map_or(pid.0, |a| a.player.0),
        )
    }

    // -----------------------------------------------------------------------------------------
    // Ticking
    // -----------------------------------------------------------------------------------------

    /// Run one net tick: [`TICKS_PER_NET_TICK`] physics ticks, then per-client output.
    pub fn net_tick(&mut self) {
        for _ in 0..TICKS_PER_NET_TICK {
            self.physics_tick();
        }
        self.net_ticks += 1;
        let tick = self.sim.tick();

        let events = self.roles.update(tick);
        self.apply_role_events(events);

        let expired: Vec<PlayerId> = self
            .players
            .values()
            .filter(|p| {
                let window = if p.wanted_hold {
                    self.cfg.wanted_grace_ticks
                } else {
                    self.cfg.reconnect_window_ticks
                };
                p.disconnected_at.is_some_and(|t| tick >= t + window)
            })
            .map(|p| p.id)
            .collect();
        for pid in expired {
            if self.players[&pid].wanted_hold {
                self.capture_abandoned(pid);
            }
            self.remove_player(pid);
        }
        self.update_hold_inputs();

        let states: BTreeMap<EntityId, VehicleState> = self.sim.vehicles().into_iter().collect();
        let positions: BTreeMap<PlayerId, [f32; 2]> = self
            .players
            .values()
            .filter_map(|p| {
                let s = states.get(&p.vehicle?)?;
                Some((p.id, [s.pos[0], s.pos[1]]))
            })
            .collect();
        let race_events = self.races.update(tick, |p| positions.get(&p).copied());
        self.apply_race_events(race_events);

        while self
            .incidents
            .front()
            .is_some_and(|i| tick > i.tick + 120 * TICK_HZ)
        {
            self.incidents.pop_front();
        }

        self.traffic_now = self
            .net_ticks
            .is_multiple_of(self.cfg.traffic_every_net_ticks.max(1))
            .then(|| self.sim.traffic_agents());
        self.update_weather(tick);

        let ids: Vec<PlayerId> = self.players.keys().copied().collect();
        for pid in ids {
            if self.players[&pid].conn.is_some() {
                self.client_output(pid, tick, &states);
            }
            let v = &mut self.players.get_mut(&pid).expect("player").view;
            v.bytes_window.push_back(v.bytes_this_tick);
            v.bytes_this_tick = 0;
            while v.bytes_window.len() > 60 {
                v.bytes_window.pop_front();
            }
        }
        self.pending_damage.clear();
        if let Some(tap) = self.tapped_events.as_mut() {
            tap.extend(self.pending_events.iter().map(|(_, e)| e.clone()));
        }
        self.pending_events.clear();
        self.traffic_now = None;
    }

    /// Broadcast weather when it changed: a new regime within a second, otherwise at most
    /// every [`ServerConfig::weather_interval_ticks`] (the water grid creeps continuously).
    fn update_weather(&mut self, tick: u64) {
        if !self.net_ticks.is_multiple_of(60) {
            return;
        }
        let Some(now) = self.sim.weather() else {
            return;
        };
        let send = match &self.last_weather {
            None => true,
            Some((t, last)) => {
                let regime = last.kind != now.kind || last.prev_kind != now.prev_kind;
                let due = tick >= t + self.cfg.weather_interval_ticks;
                *last != now && (regime || due)
            }
        };
        if send {
            self.broadcast(&ServerMsg::Weather(now.clone()));
            self.last_weather = Some((tick, now));
        }
    }

    fn physics_tick(&mut self) {
        let t = self.sim.tick();
        let mut missing = 0;
        for p in self.players.values_mut() {
            let Some(v) = p.vehicle else { continue };
            // Drop anything older than this tick (cannot happen normally).
            while p.inputs.first_key_value().is_some_and(|(k, _)| *k < t) {
                p.inputs.pop_first();
            }
            let input = match p.inputs.remove(&t) {
                Some(i) => {
                    p.last_input = i;
                    i
                }
                None if p.conn.is_none() => p.hold_input,
                None => {
                    if p.ever_sent_input {
                        missing += 1;
                    }
                    p.last_input
                }
            };
            self.sim.set_input(v, input);
        }
        self.stats.inputs_missing += missing;
        self.sim.step();

        let mut damage: BTreeMap<EntityId, TickDamage> = BTreeMap::new();
        let applied = self.sim.drain_applied_contacts();
        for (e, _) in &applied {
            self.mark_unclean(*e);
        }
        for (e, contacts) in applied {
            damage
                .entry(e)
                .or_default()
                .contacts
                .extend(contacts.iter().map(QContact::from_impulse));
        }
        for ev in self.sim.drain_events() {
            let (vehicle, d) = match ev {
                SimEvent::PartDetached { vehicle, group } => {
                    (vehicle, DamageEvent::PartDetached { group })
                }
                SimEvent::ComponentDamaged {
                    vehicle,
                    component,
                    health,
                } => (vehicle, DamageEvent::ComponentDamaged { component, health }),
                SimEvent::ComponentFailed { vehicle, component } => {
                    (vehicle, DamageEvent::ComponentFailed { component })
                }
                SimEvent::WheelDetached { vehicle, wheel } => {
                    (vehicle, DamageEvent::WheelDetached { wheel })
                }
                SimEvent::TotalLoss { vehicle } => (vehicle, DamageEvent::TotalLoss),
                SimEvent::TrafficViolation { vehicle, kind, .. } => {
                    if !matches!(kind, wp_contracts::ViolationKind::Speeding { .. }) {
                        self.mark_unclean(vehicle);
                    }
                    continue;
                }
                e @ (SimEvent::Police(_)
                | SimEvent::Race(_)
                | SimEvent::Economy(_)
                | SimEvent::WeatherChanged { .. }) => {
                    if let SimEvent::Police(PoliceEvent::PursuitEnded {
                        racer,
                        outcome,
                        level,
                        ..
                    }) = &e
                    {
                        // A bust of a player's racer (an abandoned car's was settled when
                        // it was captured and its owner is gone by now).
                        if outcome.is_capture() {
                            if let Some((pid, EntityKind::Racer)) = self.owned.get(racer).copied() {
                                let total_loss = *outcome == PursuitOutcome::CapturedTotalLoss;
                                self.settle_capture(pid, *level, total_loss);
                            }
                        }
                    }
                    self.pending_events.push((t, e));
                    continue;
                }
                _ => continue,
            };
            damage.entry(vehicle).or_default().events.push(d);
        }
        if !damage.is_empty() {
            let states: BTreeMap<EntityId, VehicleState> =
                self.sim.vehicles().into_iter().collect();
            for (e, d) in &damage {
                let big = d.contacts.iter().any(|c| c.impulse >= 20_000); // ≥ 2000 N·s
                if let (true, Some(s)) = (big, states.get(e)) {
                    self.incidents.push_back(IncidentRec {
                        x: s.pos[0],
                        y: s.pos[1],
                        kind: 0,
                        tick: t,
                    });
                    if self.incidents.len() > 64 {
                        self.incidents.pop_front();
                    }
                }
            }
            self.pending_damage.push((t, damage));
        }
    }

    /// A racer's car had a damaging contact or a non-speeding violation: its current race is
    /// not clean (meta's clean-race rating).
    fn mark_unclean(&mut self, e: EntityId) {
        if let Some((p, _)) = self.owned.get(&e) {
            if self.races.race_of(*p).is_some() {
                self.race_unclean.insert(*p);
            }
        }
    }

    fn kind_of(&self, e: EntityId) -> EntityKind {
        self.owned
            .get(&e)
            .map(|(_, k)| *k)
            .unwrap_or_else(|| self.sim.ai_kind(e))
    }

    /// Lattice snapshot of `e` as of now (`None` if it has no lattice).
    fn lattice_msg(&self, e: EntityId) -> Option<ServerMsg> {
        let (Some(nodes), Some(rest)) = (self.sim.lattice_nodes(e), self.sim.lattice_rest(e))
        else {
            return None;
        };
        let snapshot = LatticeSnapshot::capture(&nodes, &rest);
        // Full damage state only for dented cars: an undented replica built fresh and padded
        // to the server's solver phase already equals the server's model exactly (a restore
        // would also put it to sleep while the server's model may still be awake).
        let state = if snapshot.is_pristine() {
            None
        } else {
            self.sim.damage_state(e)
        };
        Some(ServerMsg::LatticeSnapshot {
            vehicle: e.0,
            tick: self.sim.tick(),
            snapshot,
            damage_steps: self.sim.damage_steps(e),
            state,
            provisional: !self.sim.damage_settled(e),
        })
    }

    /// Send a lattice snapshot; a provisional one is followed up once the model settles.
    fn send_lattice_msg(&mut self, pid: PlayerId, e: EntityId, msg: &ServerMsg) {
        if let ServerMsg::LatticeSnapshot { provisional, .. } = msg {
            let v = &mut self.players.get_mut(&pid).expect("p").view;
            if *provisional {
                v.unsettled.insert(e);
            } else {
                v.unsettled.remove(&e);
            }
        }
        self.send(pid, msg);
    }

    /// Queue a final snapshot for every provisional one whose model has settled since.
    fn follow_up_unsettled(&mut self, pid: PlayerId) {
        let v = &self.players[&pid].view;
        let mut done = Vec::new();
        for e in &v.unsettled {
            if !v.tracked.contains(e) || self.sim.damage_settled(*e) {
                done.push(*e);
            }
        }
        for e in done {
            let v = &mut self.players.get_mut(&pid).expect("p").view;
            v.unsettled.remove(&e);
            if v.tracked.contains(&e) {
                self.stats.lattice_followups += 1;
                self.queue_lattice(pid, e);
            }
        }
    }

    fn send_lattice(&mut self, pid: PlayerId, e: EntityId) {
        if let Some(msg) = self.lattice_msg(e) {
            self.send_lattice_msg(pid, e, &msg);
        }
    }

    fn client_output(
        &mut self,
        pid: PlayerId,
        tick: u64,
        states: &BTreeMap<EntityId, VehicleState>,
    ) {
        let role = self.roles.role(pid).unwrap_or(Role::Spectator);
        let own = self.players[&pid].vehicle;

        // 1. Damage for vehicles the client already tracks.
        let tracked: Vec<EntityId> = self.players[&pid].view.tracked.iter().copied().collect();
        let mut damage_msgs = Vec::new();
        for (t, per_vehicle) in &self.pending_damage {
            for e in &tracked {
                if let Some(d) = per_vehicle.get(e) {
                    damage_msgs.push(ServerMsg::Damage {
                        vehicle: e.0,
                        tick: *t,
                        contacts: d.contacts.clone(),
                        events: d.events.clone(),
                    });
                }
            }
        }
        for m in &damage_msgs {
            self.send(pid, m);
        }

        // 1b. Typed game events this client should see.
        let police_role = matches!(role, Role::PoliceChase | Role::PoliceCaptain);
        let events: Vec<SimEvent> = self
            .pending_events
            .iter()
            .filter(|(_, e)| match e {
                SimEvent::Police(PoliceEvent::Radio { .. }) => police_role,
                SimEvent::Economy(ev) => ev.account == self.economy_key(pid),
                _ => true,
            })
            .map(|(_, e)| e.clone())
            .collect();
        if !events.is_empty() {
            let newest = self.pending_events.last().map_or(tick, |(t, _)| *t);
            self.stats.events_sent += events.len() as u64;
            self.send(
                pid,
                &ServerMsg::Events {
                    tick: newest,
                    events,
                },
            );
        }

        // 2. Staggered lattice hashes and requested resyncs.
        let interval_net = (self.cfg.lattice_hash_interval_ticks / TICKS_PER_NET_TICK).max(1);
        for e in &tracked {
            if !(self.net_ticks + e.0 as u64).is_multiple_of(interval_net) {
                continue;
            }
            if let (Some(nodes), Some(rest)) =
                (self.sim.lattice_nodes(*e), self.sim.lattice_rest(*e))
            {
                let msg = ServerMsg::LatticeHash {
                    vehicle: e.0,
                    tick,
                    hash: lattice_hash(&nodes, &rest),
                };
                self.send(pid, &msg);
            }
        }
        self.follow_up_unsettled(pid);
        let requests =
            std::mem::take(&mut self.players.get_mut(&pid).expect("p").view.lattice_requests);
        for e in requests {
            self.stats.lattice_resyncs += 1;
            self.queue_lattice(pid, e);
        }

        // 3. Interest set.
        if role == Role::PoliceCaptain {
            self.clear_interest(pid, own);
            if self.net_ticks.is_multiple_of(30) {
                self.captain_view(pid, tick, states);
            }
        } else {
            self.update_interest(pid, own, states);
        }

        // 4. Lattice snapshots owed, as the byte credit allows.
        self.drain_lattice_queue(pid);

        // 5. Snapshot.
        self.snapshot(pid, tick, own, states);
    }

    fn clear_interest(&mut self, pid: PlayerId, own: Option<EntityId>) {
        let members: Vec<EntityId> = self.players[&pid].view.interest.members().collect();
        for e in members {
            self.send(pid, &ServerMsg::EntityLeave { entity: e.0 });
        }
        let v = &mut self.players.get_mut(&pid).expect("p").view;
        v.interest = InterestState::new();
        v.acked.clear();
        v.tracked.retain(|e| Some(*e) == own);
    }

    fn update_interest(
        &mut self,
        pid: PlayerId,
        own: Option<EntityId>,
        states: &BTreeMap<EntityId, VehicleState>,
    ) {
        let own_kind = own.map(|e| self.kind_of(e));
        let viewer = Viewer {
            vehicle: own,
            pos: own.and_then(|e| states.get(&e)).map_or([0.0; 3], |s| s.pos),
            police: own_kind == Some(EntityKind::PoliceChase),
            racer: own_kind == Some(EntityKind::Racer),
            race: self.races.race_of(pid),
        };
        let mut candidates = Vec::new();
        for (e, s) in states {
            let kind = self.kind_of(*e);
            let race = self
                .owned
                .get(e)
                .and_then(|(owner, _)| self.races.race_of(*owner));
            let subject = Subject {
                entity: *e,
                kind,
                pos: s.pos,
                race,
            };
            if let Some(c) = classify(&viewer, &subject) {
                candidates.push((*e, c));
            }
        }
        let (entered, left) = self
            .players
            .get_mut(&pid)
            .expect("p")
            .view
            .interest
            .update(&candidates);
        for e in left {
            let v = &mut self.players.get_mut(&pid).expect("p").view;
            v.acked.remove(&e);
            if Some(e) != own {
                v.tracked.remove(&e);
            }
            self.send(pid, &ServerMsg::EntityLeave { entity: e.0 });
        }
        for e in entered {
            let msg = ServerMsg::EntityEnter {
                entity: e.0,
                car_id: self.sim.car_id(e).unwrap_or_default(),
                kind: self.kind_of(e),
                owner: self.owned.get(&e).map(|(p, _)| p.0),
            };
            self.send(pid, &msg);
            self.players
                .get_mut(&pid)
                .expect("p")
                .view
                .tracked
                .insert(e);
            self.queue_lattice(pid, e);
        }
    }

    fn queue_lattice(&mut self, pid: PlayerId, e: EntityId) {
        let q = &mut self.players.get_mut(&pid).expect("p").view.lattice_queue;
        if !q.contains(&e) {
            q.push_back(e);
        }
    }

    /// Send queued lattice snapshots while the byte credit covers them. A snapshot of a
    /// dented car is up to ~3.7 kB (exact f32 of every displaced node), several entering at
    /// once would blow the down-link budget, so they wait: [`Self::snapshot`] then sends
    /// only minimal entity updates until the credit has grown to the waiting snapshot's size.
    fn drain_lattice_queue(&mut self, pid: PlayerId) {
        loop {
            let v = &self.players[&pid].view;
            let Some(&e) = v.lattice_queue.front() else {
                break;
            };
            let available = v.credit - v.bytes_this_tick as i64;
            if !v.tracked.contains(&e) {
                self.players
                    .get_mut(&pid)
                    .expect("p")
                    .view
                    .lattice_queue
                    .pop_front();
                continue;
            }
            let msg = self.lattice_msg(e);
            let size = msg.as_ref().map_or(0, |m| m.encode().len());
            // Also keep the sliding 1 s window under budget: a dented car with its full
            // damage state is up to ~10 kB, and several entering together (a reconnect)
            // would otherwise land in one second.
            let window_room = self.window_room(pid);
            let v = &mut self.players.get_mut(&pid).expect("p").view;
            if size as i64 > available || size as i64 > window_room {
                v.lattice_wait = size;
                return;
            }
            v.lattice_queue.pop_front();
            v.lattice_wait = 0;
            if let Some(m) = msg {
                self.send_lattice_msg(pid, e, &m);
            }
        }
        self.players.get_mut(&pid).expect("p").view.lattice_wait = 0;
    }

    /// Bytes client `pid` may still receive this tick without its last 60 ticks (1 s)
    /// exceeding 95 % of the down-link budget; a margin covers this tick's snapshot header.
    fn window_room(&self, pid: PlayerId) -> i64 {
        let v = &self.players[&pid].view;
        let recent: usize = v.bytes_window.iter().rev().take(59).sum();
        let cap = (self.cfg.downlink_budget_bps * 95 / 100) as i64;
        cap - recent as i64 - v.bytes_this_tick as i64 - 400
    }

    fn snapshot(
        &mut self,
        pid: PlayerId,
        tick: u64,
        own: Option<EntityId>,
        states: &BTreeMap<EntityId, VehicleState>,
    ) {
        let own_state = if self.net_ticks.is_multiple_of(2) {
            own.and_then(|e| states.get(&e)).map(OwnState::from_state)
        } else {
            None
        };
        // Bit-exact rewind state for the client's predictor, 5 Hz (≈0.9 kB each).
        let own_full =
            if own_state.is_some() && self.net_ticks.is_multiple_of(OWN_FULL_EVERY_NET_TICKS) {
                own.and_then(|e| self.sim.vehicle_snapshot(e))
            } else {
                None
            };
        // Refill at 85 % of the budget: a 1 s window can carry the saved-up credit for one
        // waiting lattice snapshot (≤ ~3.7 kB) on top of a second's refill and stay under it.
        let rate = (self.cfg.downlink_budget_bps * 85 / 100 / 60) as i64;
        let net_tick = self.net_ticks;
        // Kinematic traffic near the client's car (or the spectator camera origin), nearest
        // first. The captain works from the map and gets none.
        let viewer = own.and_then(|e| states.get(&e)).map_or([0.0; 3], |s| s.pos);
        let traffic = match &self.traffic_now {
            Some(all) if self.roles.role(pid) != Some(Role::PoliceCaptain) => {
                let r2 = self.cfg.traffic_radius_m * self.cfg.traffic_radius_m;
                let mut near: Vec<(f32, QTrafficAgent)> = all
                    .iter()
                    .filter_map(|a| {
                        let d = [a.pos[0] - viewer[0], a.pos[1] - viewer[1]];
                        let d2 = d[0] * d[0] + d[1] * d[1];
                        (d2 <= r2).then(|| (d2, QTrafficAgent::from_snapshot(a)))
                    })
                    .collect();
                near.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.id.cmp(&b.1.id)));
                near.truncate(self.cfg.max_traffic_agents);
                Some(near.into_iter().map(|(_, q)| q).collect::<Vec<_>>())
            }
            _ => None,
        };
        let traffic_bytes = traffic.as_ref().map_or(0, |t| 2 + t.len() * 16);
        let p = self.players.get_mut(&pid).expect("p");
        // input ack = newest tick the server holds contiguously from now.
        let mut input_ack = tick.saturating_sub(1);
        while p.inputs.contains_key(&(input_ack + 1)) {
            input_ack += 1;
        }
        let header = 1
            + 10
            + 10
            + 1
            + if own_state.is_some() { 113 } else { 0 }
            + 1
            + own_full.as_ref().map_or(0, |b| b.len() + 2)
            + 2
            + 1
            + traffic_bytes;
        // While a lattice snapshot waits, credit may grow to its size and entity updates get
        // only the minimum, so the snapshot goes out within a few ticks.
        let window_room = self.window_room(pid);
        let p = self.players.get_mut(&pid).expect("p");
        let waiting = p.view.lattice_wait as i64;
        p.view.credit = (p.view.credit + rate).min(rate.max(waiting + rate));
        let used_reliable = p.view.bytes_this_tick as i64;
        let allowance = if waiting > 0 {
            96
        } else {
            (p.view.credit - used_reliable - header as i64)
                .min(window_room + 400 - header as i64)
                .max(96) as usize
        };
        let mut used = 0;
        let mut entities = Vec::new();
        let mut sent = Vec::new();
        for e in p.view.interest.prioritized(net_tick) {
            let Some(s) = states.get(&e) else { continue };
            let q = QVehicle::from_state(s);
            let base = p
                .view
                .acked
                .get(&e)
                .filter(|(t, _)| tick - t <= TICK_HZ)
                .map(|(t, b)| ((tick - t) as u32, b));
            let d = DeltaVehicle::encode(e.0, &q, base);
            let mut w = ByteWriter::new();
            d.write(&mut w);
            if used + w.len() > allowance {
                break;
            }
            used += w.len();
            entities.push(d);
            sent.push((e, q));
            p.view.interest.mark_sent(e, net_tick);
        }
        p.view.sent.push_back((tick, sent));
        while p.view.sent.len() > 64 {
            p.view.sent.pop_front();
        }
        let msg = ServerMsg::Snapshot {
            tick,
            input_ack,
            own: own_state,
            own_full,
            entities,
            traffic,
        };
        self.send(pid, &msg);
        let v = &mut self.players.get_mut(&pid).expect("p").view;
        v.credit -= v.bytes_this_tick as i64;
    }

    fn captain_view(
        &mut self,
        pid: PlayerId,
        tick: u64,
        states: &BTreeMap<EntityId, VehicleState>,
    ) {
        let mut units = BTreeMap::new();
        for (e, s) in states {
            let kind = self.kind_of(*e);
            if kind == EntityKind::Traffic {
                continue;
            }
            // A racer in a tunnel / covered road is out of sight of every police eye, the map
            // included (backlog B04): keep showing where it went in (its last map entry), or
            // nothing if the captain never saw it.
            if kind.is_racer() && self.sim.covered_at(s.pos) {
                let v = &self.players.get(&pid).expect("p").view;
                if let Some(last) = v.captain_known.get(&e.0) {
                    units.insert(e.0, *last);
                }
                continue;
            }
            // Heading for display only (not simulation), so std atan2 is fine here.
            let fwd = crate::quant::rotate(s.rot, [0.0, 1.0, 0.0]);
            let yaw = (-fwd[0]).atan2(fwd[1]);
            let heading = ((yaw / std::f32::consts::TAU).rem_euclid(1.0) * 256.0) as u32 as u8;
            units.insert(
                e.0,
                MapUnit {
                    entity: e.0,
                    kind,
                    x_dm: (s.pos[0] * 10.0).round() as i32,
                    y_dm: (s.pos[1] * 10.0).round() as i32,
                    heading,
                    wanted: self.sim.wanted_level(*e),
                    order: self.unit_orders.get(e).map_or(0, unit_order_code),
                    unit: match kind {
                        EntityKind::PoliceChase => Some(PLAYER_UNIT_BASE + e.0),
                        EntityKind::AiPolice => self.sim.police_unit(*e).map(|u| u.0),
                        _ => None,
                    },
                },
            );
        }
        let v = &mut self.players.get_mut(&pid).expect("p").view;
        let full = !v.captain_synced;
        let upserts: Vec<MapUnit> = units
            .values()
            .filter(|u| full || v.captain_known.get(&u.entity) != Some(u))
            .copied()
            .collect();
        let removed: Vec<u32> = v
            .captain_known
            .keys()
            .filter(|k| !units.contains_key(k))
            .copied()
            .collect();
        v.captain_known = units;
        v.captain_synced = true;
        let incidents: Vec<Incident> = self
            .incidents
            .iter()
            .filter(|i| tick <= i.tick + 60 * TICK_HZ)
            .map(|i| Incident {
                x_dm: (i.x * 10.0).round() as i32,
                y_dm: (i.y * 10.0).round() as i32,
                kind: i.kind,
                age_s: ((tick - i.tick) / TICK_HZ).min(255) as u8,
            })
            .collect();
        let mut heat: BTreeMap<(i16, i16), u32> = BTreeMap::new();
        for i in &self.incidents {
            let cell = (
                (i.x / 100.0).floor().clamp(-32768.0, 32767.0) as i16,
                (i.y / 100.0).floor().clamp(-32768.0, 32767.0) as i16,
            );
            *heat.entry(cell).or_default() += 1;
        }
        let heat = heat
            .into_iter()
            .map(|((x, y), n)| (x, y, (n * 40).min(255) as u8))
            .collect();
        let msg = ServerMsg::CaptainView {
            tick,
            full,
            upserts,
            removed,
            incidents,
            heat,
        };
        self.send(pid, &msg);
    }
}
