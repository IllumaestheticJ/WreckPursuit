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
//! requested lattice snapshots, interest enter/leave, a 2 Hz captain view for the captain, and
//! one unreliable snapshot filled by priority up to the client's byte budget.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use wp_contracts::{
    AssistProfile, EntityId, PlayerId, Role, SimEvent, VehicleInput, VehicleState, CONTRACT_VERSION,
};
use wp_economy::{AccountStore, InMemoryStore, PlayerAccount};
use wp_math::Rng;

use crate::interest::{classify, InterestState, Subject, Viewer};
use crate::lattice::{lattice_hash, LatticeSnapshot};
use crate::netsim::NetSim;
use crate::protocol::{
    CaptainOrder, ClientMsg, DamageEvent, EntityKind, Incident, MapUnit, ServerMsg, UnitOrder,
    PROTOCOL_VERSION,
};
use crate::quant::{QContact, QVehicle};
use crate::race::{RaceEvent, RaceLobby};
use crate::roles::{RoleConfig, RoleEvent, RoleManager};
use crate::snapshot::{DeltaVehicle, OwnState};
use crate::wire::ByteWriter;
use crate::{Channel, DOWNLINK_BUDGET_BPS, TICKS_PER_NET_TICK, TICK_HZ};

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
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            max_players: 32,
            roles: RoleConfig::default(),
            racer_car: "debug_sedan".into(),
            police_car: "debug_coupe".into(),
            reconnect_window_ticks: 30 * TICK_HZ,
            downlink_budget_bps: DOWNLINK_BUDGET_BPS,
            lattice_hash_interval_ticks: 2 * TICK_HZ,
            spawn_points: grid_spawn_points(64, 8.0),
            seed: 1,
            max_input_lead_ticks: TICK_HZ,
            max_inputs_per_second: 300,
            lattice_requests_per_second: 4,
            captain_commands_per_second: 10,
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
    /// Lattice snapshots sent in response to hash mismatches.
    pub lattice_resyncs: u64,
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

#[derive(Debug)]
struct Player {
    id: PlayerId,
    name: String,
    token: u64,
    conn: Option<ConnId>,
    assists: AssistProfile,
    vehicle: Option<EntityId>,
    car_id: String,
    inputs: BTreeMap<u64, VehicleInput>,
    last_input: VehicleInput,
    ever_sent_input: bool,
    disconnected_at: Option<u64>,
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
    unit_orders: BTreeMap<EntityId, u8>,
    incidents: VecDeque<IncidentRec>,
    pending_damage: Vec<(u64, BTreeMap<EntityId, TickDamage>)>,
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

    /// Transport closed. A joined player keeps their car for the reconnect window.
    pub fn disconnect(&mut self, conn: ConnId) {
        if let Some(Some(pid)) = self.conns.remove(&conn) {
            let tick = self.sim.tick();
            if let Some(p) = self.players.get_mut(&pid) {
                if p.conn == Some(conn) {
                    p.conn = None;
                    p.disconnected_at = Some(tick);
                    p.inputs.clear();
                }
            }
        }
    }

    /// Frames to send since the last call.
    pub fn drain_outgoing(&mut self) -> Vec<Outgoing> {
        std::mem::take(&mut self.outgoing)
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
                self.join(conn, name, assists);
            }
            (None, _) => {} // must say Hello first
            (Some(_), ClientMsg::Hello { .. }) => {}
            (Some(pid), msg) => self.handle(pid, msg),
        }
    }

    fn join(&mut self, conn: ConnId, name: String, assists: AssistProfile) {
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
                conn: Some(conn),
                assists,
                vehicle: None,
                car_id: String::new(),
                inputs: BTreeMap::new(),
                last_input: VehicleInput::default(),
                ever_sent_input: false,
                disconnected_at: None,
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
    }

    fn reattach(&mut self, conn: ConnId, pid: PlayerId) {
        self.conns.insert(conn, Some(pid));
        let p = self.players.get_mut(&pid).expect("player exists");
        p.conn = Some(conn);
        p.disconnected_at = None;
        p.view = View::default();
        p.inputs.clear();
        p.last_input = VehicleInput::default();
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
        let token = self.players[&pid].token;
        let msg = ServerMsg::Welcome {
            player: pid,
            token,
            server_tick: self.sim.tick(),
            mode: self.sim.damage_mode(),
            protocol: PROTOCOL_VERSION,
            time_of_day_h: self.sim.time_of_day().hours(),
        };
        self.send(pid, &msg);
        self.send_account(pid);
    }

    /// The player's account, or an empty one if none is stored yet (lead adapter to
    /// `wp_economy::AccountStore`; replace with `Economy::open_account` + `settle_race`, NET-N7).
    fn account(&self, pid: PlayerId) -> PlayerAccount {
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
            } => {
                self.ack_snapshot(pid, ack_snapshot_tick);
                self.accept_inputs(pid, first_tick, &ClientMsg::inputs_dequantised(&inputs));
            }
            ClientMsg::RoleRequest { role } => {
                let events = self.roles.request(pid, role, tick);
                self.apply_role_events(events);
            }
            ClientMsg::CaptainCommand(payload) => self.captain_command(pid, &payload),
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
            ClientMsg::RaceJoin { race } => {
                if self.roles.role(pid) != Some(Role::Racer) {
                    self.notice(pid, "only racers can join races");
                    return;
                }
                match self.races.join(pid, race) {
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
            ClientMsg::RaceStart { race } => match self.races.start(pid, race, tick) {
                Ok(ev) => self.apply_race_events(vec![ev]),
                Err(e) => self.notice(pid, e),
            },
            ClientMsg::RaceLeave { race } => {
                if let Some(ev) = self.races.leave(pid, race) {
                    self.apply_race_events(vec![ev]);
                }
            }
            ClientMsg::Bye => self.remove_player(pid),
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

    fn captain_command(&mut self, pid: PlayerId, payload: &[u8]) {
        let tick = self.sim.tick();
        if self.roles.role(pid) != Some(Role::PoliceCaptain) {
            self.notice(pid, "only the captain can command units");
            return;
        }
        let limit = self.cfg.captain_commands_per_second;
        if !self
            .players
            .get_mut(&pid)
            .expect("player")
            .command_rate
            .allow(tick, limit)
        {
            self.notice(pid, "command rate exceeded");
            return;
        }
        // Placeholder validation until wp_police owns the payload (NET-3).
        let order = match CaptainOrder::decode(payload) {
            Ok(o) => o,
            Err(e) => {
                self.notice(pid, format!("bad command: {e}"));
                return;
            }
        };
        let live: BTreeSet<EntityId> = self.sim.vehicles().iter().map(|(e, _)| *e).collect();
        let mut targets = Vec::new();
        for u in order.units() {
            let e = EntityId(*u);
            match self.owned.get(&e) {
                Some((owner, EntityKind::PoliceChase)) => targets.push((e, *owner)),
                _ => {
                    self.notice(pid, format!("unit {u} is not a police chase unit"));
                    return;
                }
            }
        }
        let unit_order = match &order {
            CaptainOrder::MoveTo { x_dm, y_dm, .. } => UnitOrder {
                kind: 1,
                x_dm: *x_dm,
                y_dm: *y_dm,
                target: 0,
            },
            CaptainOrder::Block { x_dm, y_dm, .. } => UnitOrder {
                kind: 2,
                x_dm: *x_dm,
                y_dm: *y_dm,
                target: 0,
            },
            CaptainOrder::Pursue { target, .. } => {
                if !live.contains(&EntityId(*target)) {
                    self.notice(pid, "pursuit target does not exist");
                    return;
                }
                UnitOrder {
                    kind: 3,
                    x_dm: 0,
                    y_dm: 0,
                    target: *target,
                }
            }
            CaptainOrder::Hold { .. } => UnitOrder {
                kind: 4,
                x_dm: 0,
                y_dm: 0,
                target: 0,
            },
        };
        for (e, owner) in targets {
            self.unit_orders.insert(e, unit_order.kind);
            self.send(owner, &ServerMsg::UnitOrder(unit_order));
        }
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
                RaceEvent::StakesCharged { charges, .. } => {
                    for (p, amount) in charges {
                        self.credit(p, -amount, 0);
                    }
                }
                RaceEvent::Finished { race, results } => {
                    for r in &results {
                        let rep = if r.place == 1 { 10 } else { 0 };
                        self.credit(PlayerId(r.player), r.payout, rep);
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

    /// Server-side economy: the only place money changes. Until `wp_economy` exposes a payout
    /// API (NET-4) the net layer applies stakes and payouts to the account store directly.
    fn credit(&mut self, pid: PlayerId, cash: i64, rep: i64) {
        let mut a = self.account(pid);
        a.cash += cash;
        a.rep += rep;
        // Interim: no EconomyEvent is logged for this credit (economy's `settle_race` does
        // that; switching to it is NET-N7). An empty event list keeps the stored sequence.
        let _ = self.accounts.commit(&a, &[]);
        self.send_account(pid);
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
                p.disconnected_at
                    .is_some_and(|t| tick >= t + self.cfg.reconnect_window_ticks)
            })
            .map(|p| p.id)
            .collect();
        for pid in expired {
            self.remove_player(pid);
        }

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
                None if p.conn.is_none() => VehicleInput {
                    brake: 1.0,
                    ..Default::default()
                },
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
        for (e, contacts) in self.sim.drain_applied_contacts() {
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

    fn kind_of(&self, e: EntityId) -> EntityKind {
        self.owned
            .get(&e)
            .map(|(_, k)| *k)
            .unwrap_or_else(|| self.sim.ai_kind(e))
    }

    fn send_lattice(&mut self, pid: PlayerId, e: EntityId) {
        let (Some(nodes), Some(rest)) = (self.sim.lattice_nodes(e), self.sim.lattice_rest(e))
        else {
            return;
        };
        let msg = ServerMsg::LatticeSnapshot {
            vehicle: e.0,
            tick: self.sim.tick(),
            snapshot: LatticeSnapshot::capture(&nodes, &rest),
        };
        self.send(pid, &msg);
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
        let requests =
            std::mem::take(&mut self.players.get_mut(&pid).expect("p").view.lattice_requests);
        for e in requests {
            self.stats.lattice_resyncs += 1;
            self.send_lattice(pid, e);
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

        // 4. Snapshot.
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
            self.send_lattice(pid, e);
        }
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
        let budget = self.cfg.downlink_budget_bps / 60;
        let net_tick = self.net_ticks;
        let p = self.players.get_mut(&pid).expect("p");
        // input ack = newest tick the server holds contiguously from now.
        let mut input_ack = tick.saturating_sub(1);
        while p.inputs.contains_key(&(input_ack + 1)) {
            input_ack += 1;
        }
        let header = 1 + 10 + 10 + 1 + if own_state.is_some() { 113 } else { 0 } + 2;
        let used_reliable = p.view.bytes_this_tick;
        let allowance = budget.saturating_sub(used_reliable + header).max(96);
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
            entities,
        };
        self.send(pid, &msg);
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
                    order: self.unit_orders.get(e).copied().unwrap_or(0),
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
