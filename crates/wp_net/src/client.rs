//! The game client's network state machine, sans-IO (used natively by bots and tests, and in
//! the browser through `wp_wasm`).
//!
//! Call [`Client::connect`] once the transport is open, feed every received frame to
//! [`Client::receive`], call [`Client::update`] every frame with the local input and the
//! current time, and send what [`Client::drain_outgoing`] returns.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use wp_contracts::{
    AssistProfile, DamageMode, EntityId, HandlingEffects, InputSource, LoadedCar, ModeTable,
    PlayerId, Role, SimEvent, TimeOfDay, VehicleInput, VehicleState, CONTRACT_VERSION, PHYSICS_DT,
};

use crate::interp::{StateBuffer, INTERP_DELAY_TICKS};
use crate::lattice::{lattice_hash, ReplicaDamage};
use crate::predict::{Predictor, Reconcile};
use crate::protocol::{
    ClientMsg, DamageEvent, EntityKind, Incident, MapUnit, RaceInfo, RaceResultEntry, ServerMsg,
    PROTOCOL_VERSION,
};
use crate::quant::{quantize_input, QInput, RemoteState};
use crate::timesync::TimeSync;
use crate::{Channel, NetError, TICK_HZ};
use wp_contracts::{
    CaptainCommand, CaptainError, PoliceEvent, RaceCommand, TrafficAgentSnapshot, UnitOrder,
    WantedLevel, WeatherSnapshot,
};

/// Most inputs re-sent per `Inputs` message (covers ≈100 ms of loss).
pub const INPUT_REDUNDANCY: u64 = 24;
/// Most physics ticks predicted in one [`Client::update`] call.
pub const MAX_STEPS_PER_UPDATE: u64 = 32;
/// Most empty damage steps a replica runs to catch up in one go. Every step must run, even on
/// a settled lattice: `wp_damage` runs its solver on every 2nd step, so skipping steps shifts
/// the replica's solver phase against the server's. Tracked vehicles get a lattice hash every
/// 2 s, so real gaps are ≤ 480 steps; this only bounds a pathological jump.
pub const MAX_EMPTY_CATCHUP: u64 = 60 * TICK_HZ;
/// Longest a kinematic traffic agent is extrapolated past its last update, s.
pub const MAX_TRAFFIC_EXTRAPOLATION_S: f32 = 0.5;
/// Most typed game events kept for the caller to drain.
pub const MAX_PENDING_EVENTS: usize = 512;

/// Builds a damage replica for a car.
pub type DamageFactory = fn(&LoadedCar) -> Box<dyn ReplicaDamage>;

/// The production damage replica: `wp_damage::DamageModel`.
pub fn default_damage_factory(car: &LoadedCar) -> Box<dyn ReplicaDamage> {
    Box::new(wp_damage::DamageModel::new(&car.lattice, &car.materials))
}

/// Client configuration.
pub struct ClientConfig {
    /// Display name.
    pub name: String,
    /// Input device.
    pub input_source: InputSource,
    /// Cars the client can simulate/replicate, by car id.
    pub cars: BTreeMap<String, LoadedCar>,
    /// Mode tables by mode.
    pub modes: BTreeMap<DamageMode, ModeTable>,
    /// Damage replica constructor.
    pub damage_factory: DamageFactory,
}

/// Connection phase.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Phase {
    /// `connect` not called yet.
    Idle,
    /// Hello sent.
    Connecting,
    /// Welcome received.
    Joined,
    /// Server refused.
    Rejected(String),
}

/// A remote vehicle known to the client.
#[derive(Debug, Clone)]
pub struct RemoteVehicle {
    /// Car id.
    pub car_id: String,
    /// Kind.
    pub kind: EntityKind,
    /// Human owner.
    pub owner: Option<u32>,
    /// Received states.
    pub buffer: StateBuffer,
}

struct Replica {
    damage: Box<dyn ReplicaDamage>,
    rest: Vec<[f32; 3]>,
    /// Damage steps applied (the replica is "at" this tick).
    tick: u64,
    /// `apply_contacts` calls run on this replica since it was created.
    steps: u64,
    /// Restored from a snapshot of a dented lattice at some point. Such a replica has the
    /// server's node positions but not its internal damage state (NET-N5), so later hits
    /// may diverge; replicas built only from contacts must never mismatch.
    restored_dented: bool,
}

impl Replica {
    fn step(&mut self, contacts: &[wp_contracts::ContactImpulse], mode: &ModeTable) {
        let mut sink: Vec<SimEvent> = Vec::new();
        self.damage
            .apply_contacts(contacts, mode, PHYSICS_DT, &mut sink);
        self.steps += 1;
    }
}

/// Client-side counters (debug overlay, tests).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ClientStats {
    /// Smoothed RTT, ms.
    pub rtt_ms: f64,
    /// RTT deviation, ms.
    pub jitter_ms: f64,
    /// Own-car server states compared with the prediction.
    pub reconciles: u64,
    /// Of which beyond tolerance (rewound + replayed).
    pub mispredictions: u64,
    /// Last correction magnitude, m.
    pub last_correction_m: f32,
    /// Largest correction, m.
    pub max_correction_m: f32,
    /// Sum of squared corrections (over mispredictions), m².
    pub correction_sq_sum: f64,
    /// Hard resets of the prediction.
    pub prediction_resets: u64,
    /// Bytes received since start.
    pub bytes_in: u64,
    /// Bytes sent since start.
    pub bytes_out: u64,
    /// Down-link over the last second, bytes/s.
    pub bps_in: f64,
    /// Up-link over the last second, bytes/s.
    pub bps_out: f64,
    /// Snapshots received.
    pub snapshots: u64,
    /// Snapshot entities skipped because their delta baseline was missing.
    pub missing_baselines: u64,
    /// Lattice hashes compared.
    pub hash_checks: u64,
    /// Of which mismatched.
    pub hash_mismatches: u64,
    /// Of those, on replicas built only from replicated contacts (never restored from a dented
    /// snapshot). Should stay 0: contact replication is bit-exact.
    pub hash_mismatches_contact_only: u64,
    /// Lattice snapshots applied after a mismatch.
    pub resyncs: u64,
    /// Longest time from a mismatch to its resync, server ticks.
    pub max_resync_ticks: u64,
    /// Frames that failed to decode.
    pub decode_errors: u64,
}

impl ClientStats {
    /// RMS of correction magnitudes over mispredictions, m (0 if none).
    pub fn correction_rms_m(&self) -> f64 {
        if self.mispredictions == 0 {
            0.0
        } else {
            (self.correction_sq_sum / self.mispredictions as f64).sqrt()
        }
    }
}

/// Captain map state.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CaptainMap {
    /// Tick of the last update.
    pub tick: u64,
    /// Units and suspects.
    pub units: BTreeMap<u32, MapUnit>,
    /// Recent incidents.
    pub incidents: Vec<Incident>,
    /// Heat cells `(x, y, heat)` of 100 m.
    pub heat: Vec<(i16, i16, u8)>,
}

/// A vehicle to draw this frame.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderVehicle {
    /// Entity.
    pub entity: u32,
    /// Car id.
    pub car_id: String,
    /// Kind.
    pub kind: EntityKind,
    /// True for the local, predicted car.
    pub own: bool,
    /// Pose and motion.
    pub state: RemoteState,
}

/// Network client.
pub struct Client {
    cfg: ClientConfig,
    phase: Phase,
    player: Option<PlayerId>,
    token: u64,
    mode: DamageMode,
    time_of_day: TimeOfDay,
    role: Role,
    own_vehicle: Option<EntityId>,
    own_car: String,
    predictor: Option<Predictor>,
    visual_offset: [f32; 3],
    last_update_ms: Option<f64>,
    time: TimeSync,
    remotes: BTreeMap<u32, RemoteVehicle>,
    replicas: BTreeMap<u32, Replica>,
    pending_resync: BTreeMap<u32, u64>,
    last_snapshot_tick: u64,
    input_ack: u64,
    last_acked_sent: u64,
    outgoing: Vec<(Channel, Vec<u8>)>,
    bytes_in_window: VecDeque<(f64, usize)>,
    bytes_out_window: VecDeque<(f64, usize)>,
    stats: ClientStats,
    /// Damage events received (vehicle, event), for HUD/audio; drained by the caller.
    pub damage_events: Vec<(u32, DamageEvent)>,
    /// Pending captain offer expiry tick.
    pub captain_offer: Option<u64>,
    /// Captain map (captain role only).
    pub captain: CaptainMap,
    /// Latest order for this chase unit.
    pub unit_order: Option<UnitOrder>,
    /// Typed game events received (police / race / economy / weather), oldest first, at most
    /// [`MAX_PENDING_EVENTS`]; drained by the caller (HUD, audio, radio log).
    pub game_events: VecDeque<SimEvent>,
    /// Wanted level per racer vehicle, from `PoliceEvent::WantedLevelChanged`.
    pub wanted: BTreeMap<u32, WantedLevel>,
    /// Replies to captain commands `(seq, result)`, newest last (at most 20).
    pub captain_replies: VecDeque<(u32, Result<(), CaptainError>)>,
    /// Latest weather from the server (`None` = clear / not sent).
    pub weather: Option<WeatherSnapshot>,
    /// Meta reconnect token from the last `Welcome` (MC-6; empty without meta).
    pub meta_reconnect_token: String,
    /// Kinematic traffic near us as of `traffic_tick` (server physics tick).
    traffic: Vec<TrafficAgentSnapshot>,
    traffic_tick: u64,
    next_command_seq: u32,
    /// Known races.
    pub races: BTreeMap<u32, RaceInfo>,
    /// Race progress `(race, player) → next checkpoint`.
    pub race_progress: BTreeMap<(u32, u32), u32>,
    /// Latest results received.
    pub race_results: Option<(u32, Vec<RaceResultEntry>)>,
    /// Pending invitations `(race, from)`.
    pub invites: Vec<(u32, u32)>,
    /// Account `(cash, rep, cop_rank, bounty)`.
    pub account: (i64, i64, u32, u32),
    /// Recent server notices (newest last, at most 20).
    pub notices: VecDeque<String>,
}

impl Client {
    /// New idle client.
    pub fn new(cfg: ClientConfig) -> Self {
        Self {
            cfg,
            phase: Phase::Idle,
            player: None,
            token: 0,
            mode: DamageMode::Arcade,
            time_of_day: TimeOfDay::default(),
            role: Role::Spectator,
            own_vehicle: None,
            own_car: String::new(),
            predictor: None,
            visual_offset: [0.0; 3],
            last_update_ms: None,
            time: TimeSync::new(),
            remotes: BTreeMap::new(),
            replicas: BTreeMap::new(),
            pending_resync: BTreeMap::new(),
            last_snapshot_tick: 0,
            input_ack: 0,
            last_acked_sent: 0,
            outgoing: Vec::new(),
            bytes_in_window: VecDeque::new(),
            bytes_out_window: VecDeque::new(),
            stats: ClientStats::default(),
            damage_events: Vec::new(),
            captain_offer: None,
            captain: CaptainMap::default(),
            unit_order: None,
            game_events: VecDeque::new(),
            wanted: BTreeMap::new(),
            captain_replies: VecDeque::new(),
            weather: None,
            meta_reconnect_token: String::new(),
            traffic: Vec::new(),
            traffic_tick: 0,
            next_command_seq: 1,
            races: BTreeMap::new(),
            race_progress: BTreeMap::new(),
            race_results: None,
            invites: Vec::new(),
            account: (0, 0, 0, 0),
            notices: VecDeque::new(),
        }
    }

    /// Connection phase.
    pub fn phase(&self) -> &Phase {
        &self.phase
    }

    /// Our player id.
    pub fn player(&self) -> Option<PlayerId> {
        self.player
    }

    /// Reconnect token from the last `Welcome` (0 if none).
    pub fn token(&self) -> u64 {
        self.token
    }

    /// Current role.
    pub fn role(&self) -> Role {
        self.role
    }

    /// Our vehicle.
    pub fn own_vehicle(&self) -> Option<EntityId> {
        self.own_vehicle
    }

    /// Damage mode of the instance.
    pub fn mode(&self) -> DamageMode {
        self.mode
    }

    /// Counters.
    pub fn stats(&self) -> ClientStats {
        let mut s = self.stats.clone();
        s.rtt_ms = self.time.rtt_ms();
        s.jitter_ms = self.time.jitter_ms();
        s
    }

    /// Remote vehicles.
    pub fn remotes(&self) -> &BTreeMap<u32, RemoteVehicle> {
        &self.remotes
    }

    /// Estimated server tick at `now_ms`.
    pub fn server_tick(&self, now_ms: f64) -> Option<f64> {
        self.time.server_tick(now_ms)
    }

    /// Tick of the local prediction.
    pub fn predicted_tick(&self) -> Option<u64> {
        self.predictor.as_ref().map(|p| p.tick())
    }

    /// Full predicted own-vehicle state.
    pub fn predicted_state(&self) -> Option<VehicleState> {
        self.predictor.as_ref().map(|p| p.state())
    }

    /// Lattice hash of the replica of `vehicle` and the tick it is at.
    pub fn replica_hash(&self, vehicle: u32) -> Option<(u64, u32)> {
        self.replicas
            .get(&vehicle)
            .map(|r| (r.tick, lattice_hash(r.damage.node_positions(), &r.rest)))
    }

    /// Node positions of a replica (renderer deformation).
    pub fn replica_nodes(&self, vehicle: u32) -> Option<&[[f32; 3]]> {
        self.replicas
            .get(&vehicle)
            .map(|r| r.damage.node_positions())
    }

    /// Test hook: overwrite a replica's nodes (simulates a desync).
    pub fn corrupt_replica(&mut self, vehicle: u32, node: usize, delta: [f32; 3]) {
        if let Some(r) = self.replicas.get_mut(&vehicle) {
            let mut nodes = r.damage.node_positions().to_vec();
            if let Some(n) = nodes.get_mut(node) {
                for (axis, d) in delta.iter().enumerate() {
                    n[axis] += d;
                }
            }
            r.damage.restore_nodes(&nodes);
        }
    }

    /// Frames to send since the last call.
    pub fn drain_outgoing(&mut self) -> Vec<(Channel, Vec<u8>)> {
        std::mem::take(&mut self.outgoing)
    }

    fn send(&mut self, msg: &ClientMsg, channel: Channel, now_ms: f64) {
        let bytes = msg.encode();
        self.stats.bytes_out += bytes.len() as u64;
        self.bytes_out_window.push_back((now_ms, bytes.len()));
        self.outgoing.push((channel, bytes));
    }

    /// Start the handshake (transport is open). `reconnect_token` 0 = new player.
    pub fn connect(&mut self, now_ms: f64, reconnect_token: u64) {
        self.connect_with_join_token(now_ms, reconnect_token, "");
    }

    /// [`Client::connect`] presenting a meta matchmaking join token (MC-6), as returned by
    /// `GET /v1/matchmaking/status` (`assignment.join_token`).
    pub fn connect_with_join_token(&mut self, now_ms: f64, reconnect_token: u64, join_token: &str) {
        self.phase = Phase::Connecting;
        let msg = ClientMsg::Hello {
            protocol: PROTOCOL_VERSION,
            contract: CONTRACT_VERSION.to_string(),
            name: self.cfg.name.clone(),
            input_source: self.cfg.input_source,
            reconnect_token,
            join_token: join_token.to_string(),
        };
        self.send(&msg, Channel::Reliable, now_ms);
    }

    /// Ask for a role.
    pub fn request_role(&mut self, role: Role, now_ms: f64) {
        self.captain_offer = None;
        self.send(&ClientMsg::RoleRequest { role }, Channel::Reliable, now_ms);
    }

    /// Send a captain command; returns the sequence number its
    /// [`Client::captain_replies`] entry will carry.
    pub fn captain_command(&mut self, command: CaptainCommand, now_ms: f64) -> u32 {
        let seq = self.next_command_seq;
        self.next_command_seq = self.next_command_seq.wrapping_add(1).max(1);
        self.send(
            &ClientMsg::CaptainCommand { seq, command },
            Channel::Reliable,
            now_ms,
        );
        seq
    }

    /// Send a race lobby command (`Enter` / `Withdraw` a lobby race by id, `Ready` to start
    /// as host).
    pub fn race_command(&mut self, command: RaceCommand, now_ms: f64) {
        self.send(&ClientMsg::RaceCommand(command), Channel::Reliable, now_ms);
    }

    /// Send any lobby/other client message on the reliable channel.
    pub fn send_reliable(&mut self, msg: &ClientMsg, now_ms: f64) {
        self.send(msg, Channel::Reliable, now_ms);
    }

    /// Leave gracefully (no reconnect window).
    pub fn bye(&mut self, now_ms: f64) {
        self.send(&ClientMsg::Bye, Channel::Reliable, now_ms);
    }

    fn own_effects(&self) -> HandlingEffects {
        self.own_vehicle
            .and_then(|v| self.replicas.get(&v.0))
            .map(|r| r.damage.handling_effects())
            .unwrap_or(HandlingEffects::NEUTRAL)
    }

    fn assists(&self) -> AssistProfile {
        AssistProfile::for_source(self.cfg.input_source)
    }

    /// Per-frame update: ping, predict the own vehicle up to the target tick with `input`,
    /// send inputs (with redundancy) and the snapshot ack.
    pub fn update(&mut self, now_ms: f64, input: &VehicleInput) {
        if self.phase != Phase::Joined {
            return;
        }
        let dt_ms = self.last_update_ms.map_or(0.0, |t| (now_ms - t).max(0.0));
        self.last_update_ms = Some(now_ms);
        let decay = (-(dt_ms as f32) / 100.0).exp();
        self.visual_offset = self.visual_offset.map(|o| o * decay);

        if let Some(client_ms) = self.time.ping_due(now_ms) {
            self.send(&ClientMsg::Ping { client_ms }, Channel::Unreliable, now_ms);
        }
        self.trim_windows(now_ms);

        let mut inputs = Vec::new();
        let mut first_tick = 0;
        if let (Some(est), Some(pred)) = (self.time.server_tick(now_ms), self.predictor.as_mut()) {
            let target = (est + self.time.lead_ticks()).floor().max(0.0) as u64;
            let q = quantize_input(input);
            let effects = self
                .own_vehicle
                .and_then(|v| self.replicas.get(&v.0))
                .map(|r| r.damage.handling_effects())
                .unwrap_or(HandlingEffects::NEUTRAL);
            if pred.tick() + crate::predict::INPUT_RING as u64 <= target
                || pred.tick() > target + TICK_HZ
            {
                pred.warp(target);
                self.stats.prediction_resets += 1;
            }
            let steps = target.saturating_sub(pred.tick()).min(MAX_STEPS_PER_UPDATE);
            for _ in 0..steps {
                pred.step(q, &effects);
            }
            // Re-send everything the server has not confirmed, up to the redundancy window.
            let now_tick = pred.tick();
            let first = (self.input_ack + 1).max(now_tick.saturating_sub(INPUT_REDUNDANCY));
            for t in first..now_tick {
                match pred.input_at(t) {
                    Some(i) => inputs.push(QInput::from_input(&i)),
                    None => inputs.clear(), // gap after a warp: start after it
                }
            }
            first_tick = now_tick - inputs.len() as u64;
        }
        // Always send (even with no inputs): the snapshot ack drives delta baselines.
        if !inputs.is_empty() || self.last_snapshot_tick != self.last_acked_sent {
            self.last_acked_sent = self.last_snapshot_tick;
            let msg = ClientMsg::Inputs {
                ack_snapshot_tick: self.last_snapshot_tick,
                first_tick,
                inputs,
            };
            self.send(&msg, Channel::Unreliable, now_ms);
        }
    }

    fn trim_windows(&mut self, now_ms: f64) {
        for w in [&mut self.bytes_in_window, &mut self.bytes_out_window] {
            while w.front().is_some_and(|(t, _)| now_ms - t > 1000.0) {
                w.pop_front();
            }
        }
        self.stats.bps_in = self.bytes_in_window.iter().map(|(_, n)| *n).sum::<usize>() as f64;
        self.stats.bps_out = self.bytes_out_window.iter().map(|(_, n)| *n).sum::<usize>() as f64;
    }

    /// Handle one received frame.
    pub fn receive(&mut self, bytes: &[u8], now_ms: f64) -> Result<(), NetError> {
        self.stats.bytes_in += bytes.len() as u64;
        self.bytes_in_window.push_back((now_ms, bytes.len()));
        let msg = match ServerMsg::decode(bytes) {
            Ok(m) => m,
            Err(e) => {
                self.stats.decode_errors += 1;
                return Err(e);
            }
        };
        match msg {
            ServerMsg::Welcome {
                player,
                token,
                mode,
                time_of_day_h,
                meta_reconnect_token,
                ..
            } => {
                self.phase = Phase::Joined;
                self.player = Some(player);
                self.token = token;
                self.meta_reconnect_token = meta_reconnect_token;
                self.traffic.clear();
                self.mode = mode;
                self.time_of_day = TimeOfDay::from_hours(time_of_day_h);
                // A (re)join starts from a clean slate; the server re-sends everything.
                self.remotes.clear();
                self.replicas.clear();
                self.pending_resync.clear();
                self.predictor = None;
                self.last_snapshot_tick = 0;
                self.input_ack = 0;
            }
            ServerMsg::Reject { reason } => self.phase = Phase::Rejected(reason),
            ServerMsg::RoleAssigned {
                role,
                vehicle,
                car_id,
                ..
            } => {
                self.role = role;
                if vehicle != self.own_vehicle || car_id != self.own_car {
                    if let Some(old) = self.own_vehicle {
                        self.replicas.remove(&old.0);
                    }
                    self.own_vehicle = vehicle;
                    self.own_car = car_id;
                    self.predictor = None;
                    self.visual_offset = [0.0; 3];
                }
                if role != Role::PoliceCaptain {
                    self.captain = CaptainMap::default();
                }
                if role != Role::PoliceChase {
                    self.unit_order = None;
                }
            }
            ServerMsg::CaptainOffer { expires_tick } => self.captain_offer = Some(expires_tick),
            ServerMsg::EntityEnter {
                entity,
                car_id,
                kind,
                owner,
            } => {
                self.remotes.insert(
                    entity,
                    RemoteVehicle {
                        car_id,
                        kind,
                        owner,
                        buffer: StateBuffer::new(),
                    },
                );
            }
            ServerMsg::EntityLeave { entity } => {
                self.remotes.remove(&entity);
                if Some(EntityId(entity)) != self.own_vehicle {
                    self.replicas.remove(&entity);
                    self.pending_resync.remove(&entity);
                }
            }
            ServerMsg::Snapshot {
                tick,
                input_ack,
                own,
                own_full,
                entities,
                traffic,
            } => {
                self.stats.snapshots += 1;
                // Unreliable: only a newer traffic set replaces the current one.
                if let Some(list) = traffic {
                    if tick >= self.traffic_tick {
                        self.traffic = list.iter().map(|q| q.to_snapshot()).collect();
                        self.traffic_tick = tick;
                    }
                }
                self.last_snapshot_tick = self.last_snapshot_tick.max(tick);
                self.input_ack = self.input_ack.max(input_ack);
                for d in entities {
                    let Some(rv) = self.remotes.get_mut(&d.entity) else {
                        continue;
                    };
                    let base = if d.baseline_age > 0 {
                        match tick
                            .checked_sub(d.baseline_age as u64)
                            .and_then(|t| rv.buffer.at(t))
                        {
                            Some(b) => Some(*b),
                            None => {
                                self.stats.missing_baselines += 1;
                                continue;
                            }
                        }
                    } else {
                        None
                    };
                    if let Ok(q) = d.apply(base.as_ref()) {
                        rv.buffer.push(tick, q);
                    }
                }
                if let Some(own) = own {
                    self.on_own_state(tick, &own.to_state(), own_full.as_deref());
                }
            }
            ServerMsg::Damage {
                vehicle,
                tick,
                contacts,
                events,
            } => {
                let mode = self.cfg.modes.get(&self.mode).cloned();
                if let (Some(r), Some(mode)) = (self.replicas.get_mut(&vehicle), mode) {
                    if tick >= r.tick {
                        advance_empty(r, tick, &mode);
                        let impulses: Vec<_> = contacts.iter().map(|c| c.to_impulse()).collect();
                        r.step(&impulses, &mode);
                        r.tick = tick + 1;
                    }
                }
                self.damage_events
                    .extend(events.into_iter().map(|e| (vehicle, e)));
                if self.damage_events.len() > 256 {
                    let excess = self.damage_events.len() - 256;
                    self.damage_events.drain(..excess);
                }
            }
            ServerMsg::LatticeHash {
                vehicle,
                tick,
                hash,
            } => {
                let mode = self.cfg.modes.get(&self.mode).cloned();
                if let (Some(r), Some(mode)) = (self.replicas.get_mut(&vehicle), mode) {
                    if tick >= r.tick {
                        advance_empty(r, tick, &mode);
                        self.stats.hash_checks += 1;
                        if lattice_hash(r.damage.node_positions(), &r.rest) != hash {
                            self.stats.hash_mismatches += 1;
                            if !r.restored_dented {
                                self.stats.hash_mismatches_contact_only += 1;
                            }
                            if let std::collections::btree_map::Entry::Vacant(slot) =
                                self.pending_resync.entry(vehicle)
                            {
                                slot.insert(tick);
                                self.send(
                                    &ClientMsg::LatticeRequest { vehicle },
                                    Channel::Reliable,
                                    now_ms,
                                );
                            }
                        }
                    }
                }
            }
            ServerMsg::LatticeSnapshot {
                vehicle,
                tick,
                snapshot,
                damage_steps,
            } => self.on_lattice_snapshot(vehicle, tick, &snapshot, damage_steps),
            ServerMsg::Pong {
                client_ms,
                server_tick,
            } => self.time.on_pong(now_ms, client_ms, server_tick),
            ServerMsg::CaptainView {
                tick,
                full,
                upserts,
                removed,
                incidents,
                heat,
            } => {
                if full {
                    self.captain.units.clear();
                }
                self.captain.tick = tick;
                for u in upserts {
                    self.captain.units.insert(u.entity, u);
                }
                for r in removed {
                    self.captain.units.remove(&r);
                }
                self.captain.incidents = incidents;
                self.captain.heat = heat;
            }
            ServerMsg::UnitOrder(o) => self.unit_order = Some(o),
            ServerMsg::RaceUpdate(info) => {
                self.races.insert(info.race, info);
            }
            ServerMsg::RaceProgress {
                race,
                player,
                next_checkpoint,
            } => {
                self.race_progress.insert((race, player), next_checkpoint);
            }
            ServerMsg::RaceResults { race, entries } => {
                self.races.remove(&race);
                self.race_progress.retain(|(r, _), _| *r != race);
                self.race_results = Some((race, entries));
            }
            ServerMsg::RaceInvited { race, from } => self.invites.push((race, from)),
            ServerMsg::Account {
                cash,
                rep,
                cop_rank,
                bounty,
            } => self.account = (cash, rep, cop_rank, bounty),
            ServerMsg::Notice { text } => {
                self.notices.push_back(text);
                while self.notices.len() > 20 {
                    self.notices.pop_front();
                }
            }
            ServerMsg::Events { events, .. } => {
                for e in events {
                    if let SimEvent::Police(PoliceEvent::WantedLevelChanged { racer, level }) = &e {
                        self.wanted.insert(racer.0, *level);
                    }
                    self.game_events.push_back(e);
                }
                while self.game_events.len() > MAX_PENDING_EVENTS {
                    self.game_events.pop_front();
                }
            }
            ServerMsg::CaptainReply { seq, result } => {
                self.captain_replies.push_back((seq, result));
                while self.captain_replies.len() > 20 {
                    self.captain_replies.pop_front();
                }
            }
            ServerMsg::Weather(w) => self.weather = Some(w),
        }
        Ok(())
    }

    fn on_own_state(&mut self, tick: u64, state: &VehicleState, exact: Option<&[u8]>) {
        let effects = self.own_effects();
        let assists = self.assists();
        let time = self.time_of_day;
        let Some(pred) = self.predictor.as_mut() else {
            if let Some(car) = self.cfg.cars.get(&self.own_car) {
                let mut pred = Predictor::new(car, assists, state, tick, time);
                if let Some(bytes) = exact {
                    pred.restore_exact(bytes);
                }
                self.predictor = Some(pred);
            }
            return;
        };
        let before = pred.state().pos;
        match pred.reconcile(tick, state, exact, &effects) {
            Reconcile::InSync { .. } => self.stats.reconciles += 1,
            Reconcile::Corrected { correction_m, .. } => {
                self.stats.reconciles += 1;
                self.stats.mispredictions += 1;
                self.stats.last_correction_m = correction_m;
                self.stats.max_correction_m = self.stats.max_correction_m.max(correction_m);
                self.stats.correction_sq_sum += (correction_m as f64).powi(2);
                let after = pred.state().pos;
                for i in 0..3 {
                    self.visual_offset[i] += before[i] - after[i];
                }
            }
            Reconcile::Reset => self.stats.prediction_resets += 1,
            Reconcile::Stale => {}
        }
    }

    fn on_lattice_snapshot(
        &mut self,
        vehicle: u32,
        tick: u64,
        snapshot: &crate::lattice::LatticeSnapshot,
        damage_steps: Option<u64>,
    ) {
        if !self.replicas.contains_key(&vehicle) {
            let car_id = if Some(EntityId(vehicle)) == self.own_vehicle {
                Some(self.own_car.clone())
            } else {
                self.remotes.get(&vehicle).map(|r| r.car_id.clone())
            };
            let Some(car) = car_id.and_then(|c| self.cfg.cars.get(&c)) else {
                return;
            };
            let damage = (self.cfg.damage_factory)(car);
            let rest = damage.rest_positions();
            self.replicas.insert(
                vehicle,
                Replica {
                    damage,
                    rest,
                    tick,
                    steps: 0,
                    restored_dented: false,
                },
            );
        }
        let r = self.replicas.get_mut(&vehicle).expect("inserted above");
        // Match the server model's solver phase: pad with empty steps (their effect on the
        // nodes is overwritten just below).
        if let (Some(server), Some(mode)) = (damage_steps, self.cfg.modes.get(&self.mode)) {
            let p = r.damage.solver_period();
            let pad = (server % p + p - r.steps % p) % p;
            for _ in 0..pad {
                r.step(&[], mode);
            }
        }
        r.restored_dented |= !snapshot.is_pristine();
        let nodes = snapshot.positions(&r.rest);
        r.damage.restore_nodes(&nodes);
        r.tick = tick;
        if let Some(at) = self.pending_resync.remove(&vehicle) {
            self.stats.resyncs += 1;
            self.stats.max_resync_ticks = self.stats.max_resync_ticks.max(tick.saturating_sub(at));
        }
    }

    /// Everything to draw at `now_ms`: the predicted own car (smoothed) and remote cars
    /// interpolated [`INTERP_DELAY_TICKS`] behind the estimated server tick.
    pub fn render(&self, now_ms: f64) -> Vec<RenderVehicle> {
        let mut out = Vec::new();
        if let (Some(v), Some(p)) = (self.own_vehicle, &self.predictor) {
            let s = p.state();
            let pos = [0, 1, 2].map(|i| s.pos[i] + self.visual_offset[i]);
            let kind = if self.role == Role::PoliceChase {
                EntityKind::PoliceChase
            } else {
                EntityKind::Racer
            };
            out.push(RenderVehicle {
                entity: v.0,
                car_id: self.own_car.clone(),
                kind,
                own: true,
                state: RemoteState {
                    pos,
                    rot: s.rot,
                    vel: s.lin_vel,
                    ang_vel: s.ang_vel,
                    speed_mps: s.speed_mps,
                    gear: s.gear,
                    engine_rpm: s.engine_rpm,
                    health: s.health,
                    steer_rad: s.wheel_states[0].steer_rad,
                    wheels_detached: s.wheel_states.map(|w| w.detached),
                },
            });
        }
        let Some(est) = self.time.server_tick(now_ms) else {
            return out;
        };
        let render_tick = est - INTERP_DELAY_TICKS;
        for (e, rv) in &self.remotes {
            if Some(EntityId(*e)) == self.own_vehicle {
                continue;
            }
            if let Some(state) = rv.buffer.sample(render_tick) {
                out.push(RenderVehicle {
                    entity: *e,
                    car_id: rv.car_id.clone(),
                    kind: rv.kind,
                    own: false,
                    state,
                });
            }
        }
        out
    }

    /// Kinematic traffic as last received (server positions at [`Client::traffic_tick`]).
    pub fn traffic(&self) -> &[TrafficAgentSnapshot] {
        &self.traffic
    }

    /// Server physics tick of the last traffic update (0 = none yet).
    pub fn traffic_tick(&self) -> u64 {
        self.traffic_tick
    }

    /// Kinematic traffic to draw at `now_ms`: each agent moved along its heading at its speed
    /// from the update tick to the interpolation tick (≤ [`MAX_TRAFFIC_EXTRAPOLATION_S`]).
    /// Display only; agents are straight-line extrapolated, so they cut corners slightly.
    pub fn render_traffic(&self, now_ms: f64) -> Vec<TrafficAgentSnapshot> {
        let dt = self.time.server_tick(now_ms).map_or(0.0, |est| {
            let ticks = est - INTERP_DELAY_TICKS - self.traffic_tick as f64;
            (ticks / TICK_HZ as f64).clamp(0.0, MAX_TRAFFIC_EXTRAPOLATION_S as f64) as f32
        });
        self.traffic
            .iter()
            .map(|a| {
                // Display only (not simulation): std trig is fine. Yaw 0 = +Y, CCW positive.
                let (sn, cs) = a.yaw.sin_cos();
                let mut out = *a;
                out.pos[0] -= sn * a.speed_mps * dt;
                out.pos[1] += cs * a.speed_mps * dt;
                out
            })
            .collect()
    }

    /// Entities with an outstanding resync request.
    pub fn pending_resyncs(&self) -> BTreeSet<u32> {
        self.pending_resync.keys().copied().collect()
    }
}

/// Run empty damage steps until the replica is at `tick` (at most [`MAX_EMPTY_CATCHUP`]).
fn advance_empty(r: &mut Replica, tick: u64, mode: &ModeTable) {
    let gap = tick.saturating_sub(r.tick);
    for _ in 0..gap.min(MAX_EMPTY_CATCHUP) {
        r.step(&[], mode);
    }
    r.tick = tick;
}
