//! Wire messages and their binary codec. Every message is one transport frame:
//! `u8 type tag` followed by the body described on each variant (see also the README tables).
//!
//! Since contract v0.3.0 (NET-1) this module is the source of truth for the **wire**; the
//! contract's `wp_contracts::{ClientToServer, ServerToClient}` are the *logical* messages and
//! map onto these: `Inputs` carries the contract's `Input`, `RoleRequest` / `RoleAssigned` /
//! `CaptainCommand` / `CaptainReply` / `RaceCommand` / `Weather` match one-to-one, and
//! `Snapshot` (incl. kinematic `traffic`) + `Events` + `EntityEnter` + `LatticeSnapshot`
//! together are the contract's `Snapshot` / `Baseline`.
//!
//! Contract types are carried as `typed<T>` = `varint len, postcard(T)` (see
//! [`crate::wire::ByteWriter::typed`]): police / race / economy / weather events, captain and
//! race commands, captain replies and unit orders. Their layout follows the contract, which
//! is why `Hello` checks `CONTRACT_VERSION` as well as [`PROTOCOL_VERSION`].

use wp_contracts::{
    CaptainCommand, CaptainError, Component, DamageMode, EntityId, InputSource, PlayerId,
    RaceCommand, Role, SimEvent, UnitOrder, VehicleInput, WeatherKind, WeatherSnapshot,
};

use crate::lattice::LatticeSnapshot;
use crate::quant::{QContact, QInput, QTrafficAgent};
use crate::snapshot::{DeltaVehicle, OwnState};
use crate::wire::{ByteReader, ByteWriter};
use crate::NetError;

/// Protocol version; bumped on any wire change. Checked in `Hello`.
pub const PROTOCOL_VERSION: u16 = 5;

/// Most entity ids one `Inputs` message may report as missing a baseline.
pub const MAX_MISSING_PER_MSG: usize = 64;

/// Largest damage state a `LatticeSnapshot` may carry, bytes.
pub const MAX_DAMAGE_STATE_BYTES: usize = 64 * 1024;
/// Longest accepted player name, bytes.
pub const MAX_NAME_LEN: usize = 32;
/// Longest accepted meta join / reconnect token, bytes (MC-6).
pub const MAX_TOKEN_LEN: usize = 512;
/// Longest accepted `typed<CaptainCommand>` / `typed<RaceCommand>` payload, bytes.
pub const MAX_COMMAND_LEN: usize = 256;
/// Longest accepted `typed<SimEvent>` payload, bytes (radio lines, race results).
pub const MAX_EVENT_LEN: usize = 8192;
/// Most kinematic traffic agents in one snapshot.
pub const MAX_TRAFFIC_AGENTS: usize = 256;
/// Most cells in a weather water grid.
pub const MAX_WEATHER_CELLS: usize = 1 << 20;
/// Most inputs one `Inputs` message may carry (redundancy window).
pub const MAX_INPUTS_PER_MSG: usize = 32;
/// Largest accepted own-vehicle snapshot blob, bytes.
pub const MAX_VEHICLE_SNAPSHOT: usize = 4096;
/// Most checkpoints in one race.
pub const MAX_CHECKPOINTS: usize = 64;

/// Kind of replicated entity (drives interest priority and rendering).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EntityKind {
    /// Player racer.
    Racer = 0,
    /// Player-driven police chase unit.
    PoliceChase = 1,
    /// AI police unit.
    AiPolice = 2,
    /// Civilian traffic.
    Traffic = 3,
    /// AI opponent racer (`wp_sim::VehicleKind::AiRacer`).
    AiRacer = 4,
}

impl EntityKind {
    fn from_u8(v: u8) -> Result<Self, NetError> {
        Ok(match v {
            0 => Self::Racer,
            1 => Self::PoliceChase,
            2 => Self::AiPolice,
            3 => Self::Traffic,
            4 => Self::AiRacer,
            _ => return Err(NetError::Decode(format!("entity kind {v}"))),
        })
    }

    /// True for police (player or AI).
    pub fn is_police(self) -> bool {
        matches!(self, Self::PoliceChase | Self::AiPolice)
    }

    /// True for racers (player or AI).
    pub fn is_racer(self) -> bool {
        matches!(self, Self::Racer | Self::AiRacer)
    }
}

/// Encode a role as one byte.
pub fn role_to_u8(r: Role) -> u8 {
    match r {
        Role::Racer => 0,
        Role::PoliceChase => 1,
        Role::PoliceCaptain => 2,
        Role::Spectator => 3,
    }
}

/// Decode a role byte.
pub fn role_from_u8(v: u8) -> Result<Role, NetError> {
    Ok(match v {
        0 => Role::Racer,
        1 => Role::PoliceChase,
        2 => Role::PoliceCaptain,
        3 => Role::Spectator,
        _ => return Err(NetError::Decode(format!("role {v}"))),
    })
}

fn source_to_u8(s: InputSource) -> u8 {
    match s {
        InputSource::Keyboard => 0,
        InputSource::Gamepad => 1,
        InputSource::Wheel => 2,
    }
}

fn source_from_u8(v: u8) -> Result<InputSource, NetError> {
    Ok(match v {
        0 => InputSource::Keyboard,
        1 => InputSource::Gamepad,
        2 => InputSource::Wheel,
        _ => return Err(NetError::Decode(format!("input source {v}"))),
    })
}

fn mode_to_u8(m: DamageMode) -> u8 {
    match m {
        DamageMode::Arcade => 0,
        DamageMode::Simulation => 1,
    }
}

fn mode_from_u8(v: u8) -> Result<DamageMode, NetError> {
    Ok(match v {
        0 => DamageMode::Arcade,
        1 => DamageMode::Simulation,
        _ => return Err(NetError::Decode(format!("damage mode {v}"))),
    })
}

/// Wire index of each `Component` (u8 = position in this table).
const COMPONENTS: [Component; 21] = [
    Component::Engine,
    Component::Radiator,
    Component::Battery,
    Component::FuelTank,
    Component::SteeringRack,
    Component::SuspensionFL,
    Component::SuspensionFR,
    Component::SuspensionRL,
    Component::SuspensionRR,
    Component::WheelFL,
    Component::WheelFR,
    Component::WheelRL,
    Component::WheelRR,
    Component::Gearbox,
    Component::Driveshaft,
    Component::BrakeFL,
    Component::BrakeFR,
    Component::BrakeRL,
    Component::BrakeRR,
    Component::Exhaust,
    Component::Lights,
];

fn component_to_u8(c: Component) -> u8 {
    COMPONENTS.iter().position(|x| *x == c).unwrap_or(0) as u8
}

fn component_from_u8(v: u8) -> Result<Component, NetError> {
    COMPONENTS
        .get(v as usize)
        .copied()
        .ok_or_else(|| NetError::Decode(format!("component {v}")))
}

fn opt_u32(w: &mut ByteWriter, v: Option<u32>) {
    w.varint(v.map_or(0, |x| x as u64 + 1));
}

fn read_opt_u32(r: &mut ByteReader) -> Result<Option<u32>, NetError> {
    Ok(match r.varint()? {
        0 => None,
        n => Some(
            u32::try_from(n - 1).map_err(|_| NetError::Decode("option<u32> overflow".into()))?,
        ),
    })
}

/// Damage-relevant event replicated alongside contacts (for HUD/audio; replicas also derive
/// them by re-running contacts).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DamageEvent {
    /// `SimEvent::PartDetached`.
    PartDetached {
        /// Detachable group id.
        group: u16,
    },
    /// `SimEvent::ComponentDamaged` (health quantised to 1/255).
    ComponentDamaged {
        /// Component.
        component: Component,
        /// Health 0..=1.
        health: f32,
    },
    /// `SimEvent::ComponentFailed`.
    ComponentFailed {
        /// Component.
        component: Component,
    },
    /// `SimEvent::WheelDetached`.
    WheelDetached {
        /// Wheel index.
        wheel: u8,
    },
    /// `SimEvent::TotalLoss`.
    TotalLoss,
}

impl DamageEvent {
    fn write(&self, w: &mut ByteWriter) {
        match *self {
            Self::PartDetached { group } => {
                w.u8(0);
                w.u16(group);
            }
            Self::ComponentDamaged { component, health } => {
                w.u8(1);
                w.u8(component_to_u8(component));
                w.u8((health.clamp(0.0, 1.0) * 255.0).round() as u8);
            }
            Self::ComponentFailed { component } => {
                w.u8(2);
                w.u8(component_to_u8(component));
            }
            Self::WheelDetached { wheel } => {
                w.u8(3);
                w.u8(wheel);
            }
            Self::TotalLoss => w.u8(4),
        }
    }

    fn read(r: &mut ByteReader) -> Result<Self, NetError> {
        Ok(match r.u8()? {
            0 => Self::PartDetached { group: r.u16()? },
            1 => Self::ComponentDamaged {
                component: component_from_u8(r.u8()?)?,
                health: r.u8()? as f32 / 255.0,
            },
            2 => Self::ComponentFailed {
                component: component_from_u8(r.u8()?)?,
            },
            3 => Self::WheelDetached { wheel: r.u8()? },
            4 => Self::TotalLoss,
            t => return Err(NetError::Decode(format!("damage event {t}"))),
        })
    }
}

/// One marker on the captain's map.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapUnit {
    /// Entity id.
    pub entity: u32,
    /// Entity kind.
    pub kind: EntityKind,
    /// World X, dm.
    pub x_dm: i32,
    /// World Y, dm.
    pub y_dm: i32,
    /// Heading, 256 steps per turn (0 = +Y, counter-clockwise).
    pub heading: u8,
    /// Wanted level 0..=5 (suspects).
    pub wanted: u8,
    /// Current order of a police unit ([`unit_order_code`]; 0 = none / not police).
    pub order: u8,
    /// Police unit id to address in [`CaptainCommand::AssignUnit`] (`None` for non-police).
    /// AI units use `wp_police`'s ids; player chase seats use
    /// [`PLAYER_UNIT_BASE`]` + entity`.
    pub unit: Option<u32>,
}

/// First police unit id of player-driven chase seats: the captain addresses seat `entity` as
/// `UnitId(PLAYER_UNIT_BASE + entity)`. `wp_police` allocates AI unit ids from 1 upwards, so
/// the ranges never meet in practice.
pub const PLAYER_UNIT_BASE: u32 = 0x8000_0000;

/// One-byte code of a unit order for map markers: 1 patrol, 2 pursue, 3 intercept,
/// 4 respond, 5 disengage (0 = none).
pub fn unit_order_code(o: &UnitOrder) -> u8 {
    match o {
        UnitOrder::Patrol => 1,
        UnitOrder::Pursue { .. } => 2,
        UnitOrder::Intercept { .. } => 3,
        UnitOrder::Respond { .. } => 4,
        UnitOrder::Disengage => 5,
    }
}

/// An incident marker (crash, violation) on the captain's map.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Incident {
    /// World X, dm.
    pub x_dm: i32,
    /// World Y, dm.
    pub y_dm: i32,
    /// 0 crash, 1 violation, 2 police event.
    pub kind: u8,
    /// Seconds since it happened (saturating).
    pub age_s: u8,
}

/// State of a race in the instance lobby.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RaceState {
    /// Accepting members.
    Lobby,
    /// Countdown running; starts at `start_tick`.
    Countdown,
    /// Running.
    Running,
    /// Finished (results sent).
    Finished,
}

/// Public description of a race.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RaceInfo {
    /// Race id.
    pub race: u32,
    /// Hosting player.
    pub host: u32,
    /// State.
    pub state: RaceState,
    /// Start tick (valid from Countdown on).
    pub start_tick: u64,
    /// Entry stake per racer (night races cost more).
    pub stake: i64,
    /// Laps.
    pub laps: u8,
    /// Checkpoints, world XY dm.
    pub checkpoints: Vec<[i32; 2]>,
    /// Member players.
    pub members: Vec<u32>,
}

/// One line of race results.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RaceResultEntry {
    /// Player.
    pub player: u32,
    /// Finishing place, 1-based; 0 = did not finish.
    pub place: u8,
    /// Race time in physics ticks (0 for DNF).
    pub time_ticks: u64,
    /// Cash paid out (already applied server-side).
    pub payout: i64,
}

/// Messages a client sends.
#[derive(Debug, Clone, PartialEq)]
pub enum ClientMsg {
    /// `0x01` First message: `u16 protocol, string contract_version, string name,
    /// u8 input_source, u64 reconnect_token (0 = new player), string join_token`.
    Hello {
        /// [`PROTOCOL_VERSION`] the client speaks.
        protocol: u16,
        /// `wp_contracts::CONTRACT_VERSION` the client was built with.
        contract: String,
        /// Display name.
        name: String,
        /// Input device (selects the assist preset on both sides).
        input_source: InputSource,
        /// Token from an earlier `Welcome` to reclaim a player within the reconnect window.
        reconnect_token: u64,
        /// Meta matchmaking join token (MC-6; empty = none). Required for new players when
        /// the server runs with meta ([`crate::server::ServerConfig::require_join_token`]).
        join_token: String,
    },
    /// `0x02` Inputs for consecutive physics ticks starting at `first_tick` (redundant window),
    /// plus the newest snapshot tick received (delta baseline ack): `varint ack_snapshot_tick,
    /// varint first_tick, u8 n, n × 7-byte QInput`.
    Inputs {
        /// Newest snapshot tick received.
        ack_snapshot_tick: u64,
        /// Tick of `inputs[0]`.
        first_tick: u64,
        /// Quantised inputs.
        inputs: Vec<QInput>,
        /// Entities whose last snapshot delta could not be applied (baseline missing, e.g.
        /// the `EntityEnter` was still in flight): the server forgets their baselines and
        /// sends them in full next (v5). Without this an entity freezes for that client.
        missing: Vec<u32>,
    },
    /// `0x03` `u8 role`.
    RoleRequest {
        /// Requested role.
        role: Role,
    },
    /// `0x04` `varint seq, typed<CaptainCommand>` (≤ 256 bytes). Validated by `wp_police`;
    /// answered by [`ServerMsg::CaptainReply`] with the same `seq`.
    CaptainCommand {
        /// Client-chosen sequence number echoed in the reply.
        seq: u32,
        /// The command.
        command: CaptainCommand,
    },
    /// `0x05` `u32 client_ms` (client clock, wrapping).
    Ping {
        /// Client send time, ms.
        client_ms: u32,
    },
    /// `0x06` Ask for a lattice snapshot after a hash mismatch: `varint vehicle`.
    LatticeRequest {
        /// Vehicle.
        vehicle: u32,
    },
    /// `0x07` `u8 laps, varint n, n × (zigzag x_dm, zigzag y_dm)`.
    RaceCreate {
        /// Laps.
        laps: u8,
        /// Checkpoints, dm.
        checkpoints: Vec<[i32; 2]>,
    },
    /// `0x09` `varint race, varint player`.
    RaceInvite {
        /// Race.
        race: u32,
        /// Invitee.
        player: u32,
    },
    /// `0x0C` Graceful disconnect (no reconnect window).
    Bye,
    /// `0x0D` `typed<RaceCommand>`: `Enter { event }` joins lobby race `event.0`,
    /// `Withdraw { event }` leaves it (DNF once running), `Ready` from the host starts the
    /// countdown. (Replaces v3's `0x08` join, `0x0A` start and `0x0B` leave.)
    RaceCommand(RaceCommand),
}

/// Messages the server sends. "R" = reliable channel, "U" = unreliable.
#[derive(Debug, Clone, PartialEq)]
pub enum ServerMsg {
    /// `0x81` R. `varint player, u64 token, varint server_tick, u8 mode, u16 protocol,
    /// f32 time_of_day_h, string meta_reconnect_token`.
    Welcome {
        /// Assigned player id.
        player: PlayerId,
        /// Reconnect token.
        token: u64,
        /// Current server physics tick.
        server_tick: u64,
        /// Damage mode of the instance.
        mode: DamageMode,
        /// Server protocol version.
        protocol: u16,
        /// In-game time, hours.
        time_of_day_h: f32,
        /// Meta reconnect token for rejoining after the reconnect window (MC-6; empty when
        /// the server runs without meta).
        meta_reconnect_token: String,
    },
    /// `0x82` R. `string reason`; the connection is closed after it.
    Reject {
        /// Why.
        reason: String,
    },
    /// `0x83` R. `varint player, u8 role, opt vehicle, string car_id`.
    RoleAssigned {
        /// Player.
        player: PlayerId,
        /// Role.
        role: Role,
        /// Controlled vehicle.
        vehicle: Option<EntityId>,
        /// Car id of that vehicle ("" if none).
        car_id: String,
    },
    /// `0x84` R. The captain seat is offered to this client until `expires_tick`.
    CaptainOffer {
        /// Offer expiry, server tick.
        expires_tick: u64,
    },
    /// `0x85` R. Entity entered the client's interest set: `varint entity, string car_id,
    /// u8 kind, opt owner`.
    EntityEnter {
        /// Entity.
        entity: u32,
        /// Car id.
        car_id: String,
        /// Kind.
        kind: EntityKind,
        /// Owning player, if a human drives it.
        owner: Option<u32>,
    },
    /// `0x86` R. `varint entity`.
    EntityLeave {
        /// Entity.
        entity: u32,
    },
    /// `0x87` U. `varint tick, varint input_ack, u8 has_own, [OwnState], u8 has_full,
    /// [bytes own_full], varint n, n × DeltaVehicle, u8 has_traffic, [varint k,
    /// k × QTrafficAgent]`; an agent is `varint id, 3 × zigzag cm, u16 yaw, varint cm/s,
    /// u8 flags, u8 variant`.
    Snapshot {
        /// Server physics tick of the state.
        tick: u64,
        /// Newest input tick the server holds for this client (0 = none).
        input_ack: u64,
        /// Own vehicle at full precision (every other snapshot).
        own: Option<OwnState>,
        /// Own vehicle's bit-exact `wp_vehicle::VehicleSnapshot` bytes (5 Hz, with `own`).
        own_full: Option<Vec<u8>>,
        /// Interest-filtered, prioritised, delta-encoded vehicles.
        entities: Vec<DeltaVehicle>,
        /// Kinematic traffic near the client (10 Hz, nearest first). `Some` replaces the client's whole traffic set (agents absent
        /// from it are gone); `None` keeps the previous set. Physical (promoted) agents are
        /// ordinary vehicles in `entities`.
        traffic: Option<Vec<QTrafficAgent>>,
    },
    /// `0x88` R. Contacts applied to `vehicle` at physics tick `tick`, plus resulting events:
    /// `varint vehicle, varint tick, varint n, n × QContact, varint m, m × DamageEvent`.
    Damage {
        /// Vehicle.
        vehicle: u32,
        /// Physics tick the contacts were applied in.
        tick: u64,
        /// Quantised contacts (chassis space).
        contacts: Vec<QContact>,
        /// Events.
        events: Vec<DamageEvent>,
    },
    /// `0x89` R. `varint vehicle, varint tick, u32 hash`.
    LatticeHash {
        /// Vehicle.
        vehicle: u32,
        /// Tick the hash is valid after.
        tick: u64,
        /// [`crate::lattice::lattice_hash`].
        hash: u32,
    },
    /// `0x8A` R. `varint vehicle, varint tick, LatticeSnapshot` (node bitmap + exact f32
    /// positions of displaced nodes), `varint damage_steps + 1` (0 = unknown), `u8 flags`
    /// (bit 0 = state follows, bit 1 = provisional) [+ `bytes state`] (v5).
    LatticeSnapshot {
        /// Vehicle.
        vehicle: u32,
        /// Tick.
        tick: u64,
        /// Displacements.
        snapshot: LatticeSnapshot,
        /// Damage steps the server's model has run (`None` = unknown). Lets a replica match
        /// the server's solver phase (`wp_damage` solves every `solver_divider` steps).
        damage_steps: Option<u64>,
        /// Full internal damage state of a damaged car (beam rest lengths, plastic strain,
        /// broken beams, components, detached parts; NET-N5), sparse against the pristine car
        /// ([`crate::lattice::encode_damage_state`]). `None` for undamaged cars.
        state: Option<Vec<u8>>,
        /// The server's model had an impact in progress: the replica is approximate until a
        /// final (non-provisional) snapshot follows, sent once the model settles.
        provisional: bool,
    },
    /// `0x8B` U. `u32 client_ms, varint server_tick`.
    Pong {
        /// Echoed client time.
        client_ms: u32,
        /// Server tick when the ping was handled.
        server_tick: u64,
    },
    /// `0x8C` R. Captain map delta (2 Hz): `varint tick, u8 full, varint n, n × MapUnit,
    /// varint m, m × varint removed, varint k, k × Incident, varint h, h × heat cell`.
    CaptainView {
        /// Tick.
        tick: u64,
        /// True if this replaces everything (first view / resync).
        full: bool,
        /// Added or changed units.
        upserts: Vec<MapUnit>,
        /// Removed entities.
        removed: Vec<u32>,
        /// Current incidents (always complete).
        incidents: Vec<Incident>,
        /// Heat map cells `(cell_x, cell_y, heat)` of 100 m cells (always complete).
        heat: Vec<(i16, i16, u8)>,
    },
    /// `0x8D` R. `typed<UnitOrder>`: order from the captain for this chase unit.
    UnitOrder(UnitOrder),
    /// `0x8E` R. Race created/changed.
    RaceUpdate(RaceInfo),
    /// `0x8F` R. `varint race, varint player, varint next_checkpoint`.
    RaceProgress {
        /// Race.
        race: u32,
        /// Player.
        player: u32,
        /// Index of the next checkpoint (counting across laps).
        next_checkpoint: u32,
    },
    /// `0x90` R. Final results.
    RaceResults {
        /// Race.
        race: u32,
        /// Entries, by place (DNF last).
        entries: Vec<RaceResultEntry>,
    },
    /// `0x91` R. Invitation: `varint race, varint from`.
    RaceInvited {
        /// Race.
        race: u32,
        /// Inviting player.
        from: u32,
    },
    /// `0x92` R. `zigzag cash, zigzag rep, varint cop_rank, varint bounty`.
    Account {
        /// Cash.
        cash: i64,
        /// Reputation.
        rep: i64,
        /// Police rank XP.
        cop_rank: u32,
        /// Bounty.
        bounty: u32,
    },
    /// `0x93` R. Human-readable notice (rejections, warnings).
    Notice {
        /// Text.
        text: String,
    },
    /// `0x94` R. Typed game events since the last net tick: `varint tick, varint n,
    /// n × typed<SimEvent>`. Carries `SimEvent::{Police, Race, Economy, WeatherChanged}`
    /// (damage travels in [`ServerMsg::Damage`]). Police radio lines go to police roles only;
    /// economy events only to the account's owner.
    Events {
        /// Physics tick of the newest event.
        tick: u64,
        /// Events in emission order.
        events: Vec<SimEvent>,
    },
    /// `0x95` R. `varint seq, typed<Result<(), CaptainError>>`: reply to the captain command
    /// with the same `seq`.
    CaptainReply {
        /// Sequence number of the command.
        seq: u32,
        /// `wp_police`'s verdict.
        result: Result<(), CaptainError>,
    },
    /// `0x96` R. Weather (W-5), on join and when it changes (≤ 0.2 Hz unless the regime
    /// changes): `u8 kind, f32 intensity, u8 prev_kind, f32 prev_intensity, f32 blend,
    /// f32 wind_heading_rad, varint cells, varint runs, runs × (varint len, u8 water_q8)`
    /// (the water grid is run-length encoded; kinds are `WeatherKind::code`).
    Weather(WeatherSnapshot),
}

impl ServerMsg {
    /// Channel this message is sent on.
    pub fn channel(&self) -> crate::Channel {
        match self {
            Self::Snapshot { .. } | Self::Pong { .. } => crate::Channel::Unreliable,
            _ => crate::Channel::Reliable,
        }
    }
}

fn write_xy(w: &mut ByteWriter, x: i32, y: i32) {
    w.zigzag(x as i64);
    w.zigzag(y as i64);
}

fn read_i32(r: &mut ByteReader) -> Result<i32, NetError> {
    i32::try_from(r.zigzag()?).map_err(|_| NetError::Decode("i32 overflow".into()))
}

fn read_kind(r: &mut ByteReader) -> Result<EntityKind, NetError> {
    EntityKind::from_u8(r.u8()?)
}

impl ClientMsg {
    /// Encode to one frame.
    pub fn encode(&self) -> Vec<u8> {
        let mut w = ByteWriter::new();
        match self {
            Self::Hello {
                protocol,
                contract,
                name,
                input_source,
                reconnect_token,
                join_token,
            } => {
                w.u8(0x01);
                w.u16(*protocol);
                w.string(contract);
                w.string(name);
                w.u8(source_to_u8(*input_source));
                w.u64(*reconnect_token);
                w.string(join_token);
            }
            Self::Inputs {
                ack_snapshot_tick,
                first_tick,
                inputs,
                missing,
            } => {
                w.u8(0x02);
                w.varint(*ack_snapshot_tick);
                w.varint(*first_tick);
                w.u8(inputs.len().min(MAX_INPUTS_PER_MSG) as u8);
                for i in inputs.iter().take(MAX_INPUTS_PER_MSG) {
                    w.i8(i.steer);
                    w.u8(i.throttle);
                    w.u8(i.brake);
                    w.u8(i.handbrake);
                    w.u8(i.clutch);
                    w.u8(i.buttons);
                    w.u8(i.tool_use);
                }
                let missing = &missing[..missing.len().min(MAX_MISSING_PER_MSG)];
                w.varint(missing.len() as u64);
                missing.iter().for_each(|e| w.varint(*e as u64));
            }
            Self::RoleRequest { role } => {
                w.u8(0x03);
                w.u8(role_to_u8(*role));
            }
            Self::CaptainCommand { seq, command } => {
                w.u8(0x04);
                w.varint(*seq as u64);
                w.typed(command);
            }
            Self::Ping { client_ms } => {
                w.u8(0x05);
                w.u32(*client_ms);
            }
            Self::LatticeRequest { vehicle } => {
                w.u8(0x06);
                w.varint(*vehicle as u64);
            }
            Self::RaceCreate { laps, checkpoints } => {
                w.u8(0x07);
                w.u8(*laps);
                w.varint(checkpoints.len() as u64);
                checkpoints
                    .iter()
                    .for_each(|c| write_xy(&mut w, c[0], c[1]));
            }
            Self::RaceInvite { race, player } => {
                w.u8(0x09);
                w.varint(*race as u64);
                w.varint(*player as u64);
            }
            Self::Bye => w.u8(0x0C),
            Self::RaceCommand(cmd) => {
                w.u8(0x0D);
                w.typed(cmd);
            }
        }
        w.into_bytes()
    }

    /// Decode one frame. Rejects unknown tags, oversize fields and trailing bytes.
    pub fn decode(bytes: &[u8]) -> Result<Self, NetError> {
        let mut r = ByteReader::new(bytes);
        let msg = match r.u8()? {
            0x01 => Self::Hello {
                protocol: r.u16()?,
                contract: r.string(16)?,
                name: r.string(MAX_NAME_LEN)?,
                input_source: source_from_u8(r.u8()?)?,
                reconnect_token: r.u64()?,
                join_token: r.string(MAX_TOKEN_LEN)?,
            },
            0x02 => {
                let ack_snapshot_tick = r.varint()?;
                let first_tick = r.varint()?;
                let n = r.u8()? as usize;
                if n > MAX_INPUTS_PER_MSG {
                    return Err(NetError::Decode("too many inputs".into()));
                }
                let mut inputs = Vec::with_capacity(n);
                for _ in 0..n {
                    inputs.push(QInput {
                        steer: r.i8()?,
                        throttle: r.u8()?,
                        brake: r.u8()?,
                        handbrake: r.u8()?,
                        clutch: r.u8()?,
                        buttons: r.u8()?,
                        tool_use: r.u8()?,
                    });
                }
                let m = r.len(MAX_MISSING_PER_MSG)?;
                let mut missing = Vec::with_capacity(m);
                for _ in 0..m {
                    missing.push(r.varint_u32()?);
                }
                Self::Inputs {
                    ack_snapshot_tick,
                    first_tick,
                    inputs,
                    missing,
                }
            }
            0x03 => Self::RoleRequest {
                role: role_from_u8(r.u8()?)?,
            },
            0x04 => Self::CaptainCommand {
                seq: r.varint_u32()?,
                command: r.typed(MAX_COMMAND_LEN)?,
            },
            0x05 => Self::Ping {
                client_ms: r.u32()?,
            },
            0x06 => Self::LatticeRequest {
                vehicle: r.varint_u32()?,
            },
            0x07 => {
                let laps = r.u8()?;
                let n = r.len(MAX_CHECKPOINTS)?;
                let mut checkpoints = Vec::with_capacity(n);
                for _ in 0..n {
                    checkpoints.push([read_i32(&mut r)?, read_i32(&mut r)?]);
                }
                Self::RaceCreate { laps, checkpoints }
            }
            0x09 => Self::RaceInvite {
                race: r.varint_u32()?,
                player: r.varint_u32()?,
            },
            0x0C => Self::Bye,
            0x0D => Self::RaceCommand(r.typed(MAX_COMMAND_LEN)?),
            t => {
                return Err(NetError::Decode(format!(
                    "unknown client message 0x{t:02x}"
                )))
            }
        };
        r.finish()?;
        Ok(msg)
    }

    /// The dequantised inputs of an `Inputs` message.
    pub fn inputs_dequantised(inputs: &[QInput]) -> Vec<VehicleInput> {
        inputs.iter().map(|q| q.to_input()).collect()
    }
}

fn write_contact(w: &mut ByteWriter, c: &QContact) {
    c.point.iter().for_each(|v| w.i16(*v));
    c.normal.iter().for_each(|v| w.i16(*v));
    w.varint(c.impulse as u64);
    c.tangent.iter().for_each(|v| w.zigzag(*v as i64));
    w.zigzag(c.rel_speed as i64);
    opt_u32(w, c.other.map(|e| e.0));
    w.u16(c.other_mass_kg);
}

fn read_contact(r: &mut ByteReader) -> Result<QContact, NetError> {
    let mut c = QContact::default();
    for v in &mut c.point {
        *v = r.i16()?;
    }
    for v in &mut c.normal {
        *v = r.i16()?;
    }
    c.impulse = r.varint_u32()?;
    for v in &mut c.tangent {
        *v = read_i32(r)?;
    }
    c.rel_speed = read_i32(r)?;
    c.other = read_opt_u32(r)?.map(EntityId);
    c.other_mass_kg = r.u16()?;
    Ok(c)
}

fn write_traffic(w: &mut ByteWriter, a: &QTrafficAgent) {
    w.varint(a.id as u64);
    a.pos.iter().for_each(|v| w.zigzag(*v as i64));
    w.u16(a.yaw);
    w.varint(a.speed_cms as u64);
    w.u8(a.flags);
    w.u8(a.variant);
}

fn read_traffic(r: &mut ByteReader) -> Result<QTrafficAgent, NetError> {
    Ok(QTrafficAgent {
        id: r.varint_u32()?,
        pos: [read_i32(r)?, read_i32(r)?, read_i32(r)?],
        yaw: r.u16()?,
        speed_cms: r.varint_u32()?,
        flags: r.u8()?,
        variant: r.u8()?,
    })
}

fn weather_kind(code: u8) -> Result<WeatherKind, NetError> {
    WeatherKind::from_code(code).ok_or_else(|| NetError::Decode(format!("weather kind {code}")))
}

fn write_weather(w: &mut ByteWriter, s: &WeatherSnapshot) {
    w.u8(s.kind.code());
    w.f32(s.intensity);
    w.u8(s.prev_kind.code());
    w.f32(s.prev_intensity);
    w.f32(s.blend);
    w.f32(s.wind_heading_rad);
    // Run-length encode the water grid: dry or evenly wet cities are a handful of runs.
    let mut runs: Vec<(u64, u8)> = Vec::new();
    for v in s.water_q8.iter().take(MAX_WEATHER_CELLS) {
        match runs.last_mut() {
            Some((n, last)) if *last == *v => *n += 1,
            _ => runs.push((1, *v)),
        }
    }
    w.varint(s.water_q8.len().min(MAX_WEATHER_CELLS) as u64);
    w.varint(runs.len() as u64);
    for (n, v) in runs {
        w.varint(n);
        w.u8(v);
    }
}

fn read_weather(r: &mut ByteReader) -> Result<WeatherSnapshot, NetError> {
    let kind = weather_kind(r.u8()?)?;
    let intensity = r.f32()?;
    let prev_kind = weather_kind(r.u8()?)?;
    let prev_intensity = r.f32()?;
    let blend = r.f32()?;
    let wind_heading_rad = r.f32()?;
    let cells = r.len(MAX_WEATHER_CELLS)?;
    let runs = r.len(cells)?;
    let mut water_q8 = Vec::with_capacity(cells);
    for _ in 0..runs {
        let n = r.len(cells - water_q8.len())?;
        if n == 0 {
            return Err(NetError::Decode("empty weather run".into()));
        }
        let v = r.u8()?;
        water_q8.extend(std::iter::repeat_n(v, n));
    }
    if water_q8.len() != cells {
        return Err(NetError::Decode(
            "weather runs do not cover the grid".into(),
        ));
    }
    Ok(WeatherSnapshot {
        kind,
        intensity,
        prev_kind,
        prev_intensity,
        blend,
        wind_heading_rad,
        water_q8,
    })
}

fn race_state_u8(s: RaceState) -> u8 {
    match s {
        RaceState::Lobby => 0,
        RaceState::Countdown => 1,
        RaceState::Running => 2,
        RaceState::Finished => 3,
    }
}

fn race_state_from(v: u8) -> Result<RaceState, NetError> {
    Ok(match v {
        0 => RaceState::Lobby,
        1 => RaceState::Countdown,
        2 => RaceState::Running,
        3 => RaceState::Finished,
        _ => return Err(NetError::Decode(format!("race state {v}"))),
    })
}

impl ServerMsg {
    /// Encode to one frame.
    pub fn encode(&self) -> Vec<u8> {
        let mut w = ByteWriter::new();
        self.write(&mut w);
        w.into_bytes()
    }

    fn write(&self, w: &mut ByteWriter) {
        match self {
            Self::Welcome {
                player,
                token,
                server_tick,
                mode,
                protocol,
                time_of_day_h,
                meta_reconnect_token,
            } => {
                w.u8(0x81);
                w.varint(player.0 as u64);
                w.u64(*token);
                w.varint(*server_tick);
                w.u8(mode_to_u8(*mode));
                w.u16(*protocol);
                w.f32(*time_of_day_h);
                w.string(meta_reconnect_token);
            }
            Self::Reject { reason } => {
                w.u8(0x82);
                w.string(reason);
            }
            Self::RoleAssigned {
                player,
                role,
                vehicle,
                car_id,
            } => {
                w.u8(0x83);
                w.varint(player.0 as u64);
                w.u8(role_to_u8(*role));
                opt_u32(w, vehicle.map(|e| e.0));
                w.string(car_id);
            }
            Self::CaptainOffer { expires_tick } => {
                w.u8(0x84);
                w.varint(*expires_tick);
            }
            Self::EntityEnter {
                entity,
                car_id,
                kind,
                owner,
            } => {
                w.u8(0x85);
                w.varint(*entity as u64);
                w.string(car_id);
                w.u8(*kind as u8);
                opt_u32(w, *owner);
            }
            Self::EntityLeave { entity } => {
                w.u8(0x86);
                w.varint(*entity as u64);
            }
            Self::Snapshot {
                tick,
                input_ack,
                own,
                own_full,
                entities,
                traffic,
            } => {
                w.u8(0x87);
                w.varint(*tick);
                w.varint(*input_ack);
                match own {
                    Some(o) => {
                        w.u8(1);
                        o.write(w);
                    }
                    None => w.u8(0),
                }
                match own_full {
                    Some(b) => {
                        w.u8(1);
                        w.bytes(b);
                    }
                    None => w.u8(0),
                }
                w.varint(entities.len() as u64);
                entities.iter().for_each(|e| e.write(w));
                match traffic {
                    Some(list) => {
                        w.u8(1);
                        let n = list.len().min(MAX_TRAFFIC_AGENTS);
                        w.varint(n as u64);
                        list.iter().take(n).for_each(|a| write_traffic(w, a));
                    }
                    None => w.u8(0),
                }
            }
            Self::Damage {
                vehicle,
                tick,
                contacts,
                events,
            } => {
                w.u8(0x88);
                w.varint(*vehicle as u64);
                w.varint(*tick);
                w.varint(contacts.len() as u64);
                contacts.iter().for_each(|c| write_contact(w, c));
                w.varint(events.len() as u64);
                events.iter().for_each(|e| e.write(w));
            }
            Self::LatticeHash {
                vehicle,
                tick,
                hash,
            } => {
                w.u8(0x89);
                w.varint(*vehicle as u64);
                w.varint(*tick);
                w.u32(*hash);
            }
            Self::LatticeSnapshot {
                vehicle,
                tick,
                snapshot,
                damage_steps,
                state,
                provisional,
            } => {
                w.u8(0x8A);
                w.varint(*vehicle as u64);
                w.varint(*tick);
                snapshot.encode(w);
                w.varint(damage_steps.map_or(0, |s| s + 1));
                w.u8(u8::from(state.is_some()) | (u8::from(*provisional) << 1));
                if let Some(b) = state {
                    w.bytes(b);
                }
            }
            Self::Pong {
                client_ms,
                server_tick,
            } => {
                w.u8(0x8B);
                w.u32(*client_ms);
                w.varint(*server_tick);
            }
            Self::CaptainView {
                tick,
                full,
                upserts,
                removed,
                incidents,
                heat,
            } => {
                w.u8(0x8C);
                w.varint(*tick);
                w.bool(*full);
                w.varint(upserts.len() as u64);
                for u in upserts {
                    w.varint(u.entity as u64);
                    w.u8(u.kind as u8);
                    write_xy(w, u.x_dm, u.y_dm);
                    w.u8(u.heading);
                    w.u8(u.wanted);
                    w.u8(u.order);
                    opt_u32(w, u.unit);
                }
                w.varint(removed.len() as u64);
                removed.iter().for_each(|e| w.varint(*e as u64));
                w.varint(incidents.len() as u64);
                for i in incidents {
                    write_xy(w, i.x_dm, i.y_dm);
                    w.u8(i.kind);
                    w.u8(i.age_s);
                }
                w.varint(heat.len() as u64);
                for (x, y, h) in heat {
                    w.i16(*x);
                    w.i16(*y);
                    w.u8(*h);
                }
            }
            Self::UnitOrder(o) => {
                w.u8(0x8D);
                w.typed(o);
            }
            Self::RaceUpdate(info) => {
                w.u8(0x8E);
                w.varint(info.race as u64);
                w.varint(info.host as u64);
                w.u8(race_state_u8(info.state));
                w.varint(info.start_tick);
                w.zigzag(info.stake);
                w.u8(info.laps);
                w.varint(info.checkpoints.len() as u64);
                info.checkpoints
                    .iter()
                    .for_each(|c| write_xy(w, c[0], c[1]));
                w.varint(info.members.len() as u64);
                info.members.iter().for_each(|m| w.varint(*m as u64));
            }
            Self::RaceProgress {
                race,
                player,
                next_checkpoint,
            } => {
                w.u8(0x8F);
                w.varint(*race as u64);
                w.varint(*player as u64);
                w.varint(*next_checkpoint as u64);
            }
            Self::RaceResults { race, entries } => {
                w.u8(0x90);
                w.varint(*race as u64);
                w.varint(entries.len() as u64);
                for e in entries {
                    w.varint(e.player as u64);
                    w.u8(e.place);
                    w.varint(e.time_ticks);
                    w.zigzag(e.payout);
                }
            }
            Self::RaceInvited { race, from } => {
                w.u8(0x91);
                w.varint(*race as u64);
                w.varint(*from as u64);
            }
            Self::Account {
                cash,
                rep,
                cop_rank,
                bounty,
            } => {
                w.u8(0x92);
                w.zigzag(*cash);
                w.zigzag(*rep);
                w.varint(*cop_rank as u64);
                w.varint(*bounty as u64);
            }
            Self::Notice { text } => {
                w.u8(0x93);
                w.string(text);
            }
            Self::Events { tick, events } => {
                w.u8(0x94);
                w.varint(*tick);
                w.varint(events.len() as u64);
                events.iter().for_each(|e| w.typed(e));
            }
            Self::CaptainReply { seq, result } => {
                w.u8(0x95);
                w.varint(*seq as u64);
                w.typed(result);
            }
            Self::Weather(snap) => {
                w.u8(0x96);
                write_weather(w, snap);
            }
        }
    }

    /// Decode one frame.
    pub fn decode(bytes: &[u8]) -> Result<Self, NetError> {
        let mut r = ByteReader::new(bytes);
        let msg = match r.u8()? {
            0x81 => Self::Welcome {
                player: PlayerId(r.varint_u32()?),
                token: r.u64()?,
                server_tick: r.varint()?,
                mode: mode_from_u8(r.u8()?)?,
                protocol: r.u16()?,
                time_of_day_h: r.f32()?,
                meta_reconnect_token: r.string(MAX_TOKEN_LEN)?,
            },
            0x82 => Self::Reject {
                reason: r.string(256)?,
            },
            0x83 => Self::RoleAssigned {
                player: PlayerId(r.varint_u32()?),
                role: role_from_u8(r.u8()?)?,
                vehicle: read_opt_u32(&mut r)?.map(EntityId),
                car_id: r.string(64)?,
            },
            0x84 => Self::CaptainOffer {
                expires_tick: r.varint()?,
            },
            0x85 => Self::EntityEnter {
                entity: r.varint_u32()?,
                car_id: r.string(64)?,
                kind: read_kind(&mut r)?,
                owner: read_opt_u32(&mut r)?,
            },
            0x86 => Self::EntityLeave {
                entity: r.varint_u32()?,
            },
            0x87 => {
                let tick = r.varint()?;
                let input_ack = r.varint()?;
                let own = if r.bool()? {
                    Some(OwnState::read(&mut r)?)
                } else {
                    None
                };
                let own_full = if r.bool()? {
                    Some(r.bytes(MAX_VEHICLE_SNAPSHOT)?)
                } else {
                    None
                };
                let n = r.len(4096)?;
                let mut entities = Vec::with_capacity(n);
                for _ in 0..n {
                    entities.push(DeltaVehicle::read(&mut r)?);
                }
                let traffic = if r.bool()? {
                    let k = r.len(MAX_TRAFFIC_AGENTS)?;
                    let mut list = Vec::with_capacity(k);
                    for _ in 0..k {
                        list.push(read_traffic(&mut r)?);
                    }
                    Some(list)
                } else {
                    None
                };
                Self::Snapshot {
                    tick,
                    input_ack,
                    own,
                    own_full,
                    entities,
                    traffic,
                }
            }
            0x88 => {
                let vehicle = r.varint_u32()?;
                let tick = r.varint()?;
                let n = r.len(1024)?;
                let mut contacts = Vec::with_capacity(n);
                for _ in 0..n {
                    contacts.push(read_contact(&mut r)?);
                }
                let m = r.len(1024)?;
                let mut events = Vec::with_capacity(m);
                for _ in 0..m {
                    events.push(DamageEvent::read(&mut r)?);
                }
                Self::Damage {
                    vehicle,
                    tick,
                    contacts,
                    events,
                }
            }
            0x89 => Self::LatticeHash {
                vehicle: r.varint_u32()?,
                tick: r.varint()?,
                hash: r.u32()?,
            },
            0x8A => {
                let vehicle = r.varint_u32()?;
                let tick = r.varint()?;
                let snapshot = LatticeSnapshot::decode(&mut r)?;
                let damage_steps = r.varint()?.checked_sub(1);
                let flags = r.u8()?;
                if flags > 3 {
                    return Err(NetError::Decode("lattice snapshot flags".into()));
                }
                let state = if flags & 1 != 0 {
                    Some(r.bytes(MAX_DAMAGE_STATE_BYTES)?)
                } else {
                    None
                };
                Self::LatticeSnapshot {
                    vehicle,
                    tick,
                    snapshot,
                    damage_steps,
                    state,
                    provisional: flags & 2 != 0,
                }
            }
            0x8B => Self::Pong {
                client_ms: r.u32()?,
                server_tick: r.varint()?,
            },
            0x8C => {
                let tick = r.varint()?;
                let full = r.bool()?;
                let n = r.len(4096)?;
                let mut upserts = Vec::with_capacity(n);
                for _ in 0..n {
                    upserts.push(MapUnit {
                        entity: r.varint_u32()?,
                        kind: read_kind(&mut r)?,
                        x_dm: read_i32(&mut r)?,
                        y_dm: read_i32(&mut r)?,
                        heading: r.u8()?,
                        wanted: r.u8()?,
                        order: r.u8()?,
                        unit: read_opt_u32(&mut r)?,
                    });
                }
                let m = r.len(4096)?;
                let mut removed = Vec::with_capacity(m);
                for _ in 0..m {
                    removed.push(r.varint_u32()?);
                }
                let k = r.len(1024)?;
                let mut incidents = Vec::with_capacity(k);
                for _ in 0..k {
                    incidents.push(Incident {
                        x_dm: read_i32(&mut r)?,
                        y_dm: read_i32(&mut r)?,
                        kind: r.u8()?,
                        age_s: r.u8()?,
                    });
                }
                let h = r.len(4096)?;
                let mut heat = Vec::with_capacity(h);
                for _ in 0..h {
                    heat.push((r.i16()?, r.i16()?, r.u8()?));
                }
                Self::CaptainView {
                    tick,
                    full,
                    upserts,
                    removed,
                    incidents,
                    heat,
                }
            }
            0x8D => Self::UnitOrder(r.typed(MAX_COMMAND_LEN)?),
            0x8E => {
                let race = r.varint_u32()?;
                let host = r.varint_u32()?;
                let state = race_state_from(r.u8()?)?;
                let start_tick = r.varint()?;
                let stake = r.zigzag()?;
                let laps = r.u8()?;
                let n = r.len(MAX_CHECKPOINTS)?;
                let mut checkpoints = Vec::with_capacity(n);
                for _ in 0..n {
                    checkpoints.push([read_i32(&mut r)?, read_i32(&mut r)?]);
                }
                let m = r.len(256)?;
                let mut members = Vec::with_capacity(m);
                for _ in 0..m {
                    members.push(r.varint_u32()?);
                }
                Self::RaceUpdate(RaceInfo {
                    race,
                    host,
                    state,
                    start_tick,
                    stake,
                    laps,
                    checkpoints,
                    members,
                })
            }
            0x8F => Self::RaceProgress {
                race: r.varint_u32()?,
                player: r.varint_u32()?,
                next_checkpoint: r.varint_u32()?,
            },
            0x90 => {
                let race = r.varint_u32()?;
                let n = r.len(256)?;
                let mut entries = Vec::with_capacity(n);
                for _ in 0..n {
                    entries.push(RaceResultEntry {
                        player: r.varint_u32()?,
                        place: r.u8()?,
                        time_ticks: r.varint()?,
                        payout: r.zigzag()?,
                    });
                }
                Self::RaceResults { race, entries }
            }
            0x91 => Self::RaceInvited {
                race: r.varint_u32()?,
                from: r.varint_u32()?,
            },
            0x92 => Self::Account {
                cash: r.zigzag()?,
                rep: r.zigzag()?,
                cop_rank: r.varint_u32()?,
                bounty: r.varint_u32()?,
            },
            0x93 => Self::Notice {
                text: r.string(512)?,
            },
            0x94 => {
                let tick = r.varint()?;
                let n = r.len(4096)?;
                let mut events = Vec::with_capacity(n);
                for _ in 0..n {
                    events.push(r.typed(MAX_EVENT_LEN)?);
                }
                Self::Events { tick, events }
            }
            0x95 => Self::CaptainReply {
                seq: r.varint_u32()?,
                result: r.typed(MAX_COMMAND_LEN)?,
            },
            0x96 => Self::Weather(read_weather(&mut r)?),
            t => {
                return Err(NetError::Decode(format!(
                    "unknown server message 0x{t:02x}"
                )))
            }
        };
        r.finish()?;
        Ok(msg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quant::QVehicle;

    fn rt_client(m: ClientMsg) {
        let bytes = m.encode();
        assert_eq!(ClientMsg::decode(&bytes).unwrap(), m, "{m:?}");
        // Every strict prefix fails cleanly.
        for cut in 0..bytes.len() {
            assert!(ClientMsg::decode(&bytes[..cut]).is_err());
        }
    }

    fn rt_server(m: ServerMsg) {
        let bytes = m.encode();
        assert_eq!(ServerMsg::decode(&bytes).unwrap(), m, "{m:?}");
        for cut in 0..bytes.len() {
            assert!(ServerMsg::decode(&bytes[..cut]).is_err());
        }
    }

    #[test]
    fn every_client_message_round_trips() {
        rt_client(ClientMsg::Hello {
            protocol: PROTOCOL_VERSION,
            contract: "0.1.0".into(),
            name: "bot-7".into(),
            input_source: InputSource::Gamepad,
            reconnect_token: 0xDEAD_BEEF,
            join_token: "mm-join-abc".into(),
        });
        rt_client(ClientMsg::Inputs {
            ack_snapshot_tick: 1000,
            first_tick: 996,
            inputs: vec![
                QInput::from_input(&VehicleInput {
                    steer: -0.5,
                    throttle: 1.0,
                    gear_up: true,
                    ..Default::default()
                });
                3
            ],
            missing: vec![7, 300],
        });
        rt_client(ClientMsg::RoleRequest {
            role: Role::PoliceCaptain,
        });
        for command in [
            CaptainCommand::AssignUnit {
                unit: wp_contracts::UnitId(3),
                order: UnitOrder::Pursue {
                    target: EntityId(9),
                },
            },
            CaptainCommand::RequestRoadblock {
                target: EntityId(9),
                road: Some(wp_contracts::RoadEdgeId(12)),
            },
            CaptainCommand::RequestSpikes {
                target: EntityId(9),
                road: None,
            },
            CaptainCommand::RequestHelicopter {
                target: EntityId(2),
            },
            CaptainCommand::SetPriorityTarget { target: None },
            CaptainCommand::CallOff {
                target: EntityId(2),
            },
        ] {
            rt_client(ClientMsg::CaptainCommand { seq: 41, command });
        }
        rt_client(ClientMsg::Ping { client_ms: 123456 });
        rt_client(ClientMsg::LatticeRequest { vehicle: 12 });
        rt_client(ClientMsg::RaceCreate {
            laps: 2,
            checkpoints: vec![[0, 0], [1000, -2000]],
        });
        rt_client(ClientMsg::RaceInvite { race: 1, player: 5 });
        rt_client(ClientMsg::Bye);
        let event = wp_contracts::RaceEventId(1);
        rt_client(ClientMsg::RaceCommand(RaceCommand::Enter { event }));
        rt_client(ClientMsg::RaceCommand(RaceCommand::Withdraw { event }));
        rt_client(ClientMsg::RaceCommand(RaceCommand::Ready));
    }

    #[test]
    fn every_server_message_round_trips() {
        rt_server(ServerMsg::Welcome {
            player: PlayerId(3),
            token: 99,
            server_tick: 1 << 20,
            mode: DamageMode::Simulation,
            protocol: PROTOCOL_VERSION,
            time_of_day_h: 22.5,
            meta_reconnect_token: "meta-rt-xyz".into(),
        });
        rt_server(ServerMsg::Reject {
            reason: "full".into(),
        });
        rt_server(ServerMsg::RoleAssigned {
            player: PlayerId(3),
            role: Role::PoliceChase,
            vehicle: Some(EntityId(0)),
            car_id: "debug_coupe".into(),
        });
        rt_server(ServerMsg::CaptainOffer { expires_tick: 500 });
        rt_server(ServerMsg::EntityEnter {
            entity: 7,
            car_id: "debug_sedan".into(),
            kind: EntityKind::Traffic,
            owner: None,
        });
        rt_server(ServerMsg::EntityLeave { entity: 7 });
        let q = QVehicle {
            pos: [100, 200, 3],
            rot: 77,
            ..Default::default()
        };
        rt_server(ServerMsg::Snapshot {
            tick: 4000,
            input_ack: 4010,
            own: Some(OwnState::default()),
            own_full: Some(vec![1, 2, 3, 250]),
            entities: vec![
                DeltaVehicle::encode(1, &q, None),
                DeltaVehicle::encode(2, &q, Some((8, &QVehicle::default()))),
            ],
            traffic: Some(vec![
                QTrafficAgent {
                    id: 300,
                    pos: [-12_345, 67_890, 12],
                    yaw: 40_000,
                    speed_cms: 1_250,
                    flags: 0b10_0001,
                    variant: 2,
                },
                QTrafficAgent::default(),
            ]),
        });
        rt_server(ServerMsg::Snapshot {
            tick: 4001,
            input_ack: 0,
            own: None,
            own_full: None,
            entities: vec![],
            traffic: None,
        });
        rt_server(ServerMsg::Damage {
            vehicle: 2,
            tick: 4001,
            contacts: vec![QContact {
                point: [1, -2, 3],
                normal: [0, -32767, 0],
                impulse: 12345,
                tangent: [-5, 0, 9],
                rel_speed: 1500,
                other: Some(EntityId(4)),
                other_mass_kg: 1400,
            }],
            events: vec![
                DamageEvent::PartDetached { group: 2 },
                DamageEvent::ComponentDamaged {
                    component: Component::Radiator,
                    health: 128.0 / 255.0,
                },
                DamageEvent::ComponentFailed {
                    component: Component::Lights,
                },
                DamageEvent::WheelDetached { wheel: 3 },
                DamageEvent::TotalLoss,
            ],
        });
        rt_server(ServerMsg::LatticeHash {
            vehicle: 2,
            tick: 4000,
            hash: 0xABCD_0123,
        });
        rt_server(ServerMsg::LatticeSnapshot {
            vehicle: 2,
            tick: 4000,
            snapshot: LatticeSnapshot {
                node_count: 3,
                displaced: vec![(1, [0.5, -0.25, 0.125])],
            },
            damage_steps: Some(12_345),
            state: Some(vec![1, 2, 3]),
            provisional: true,
        });
        rt_server(ServerMsg::Pong {
            client_ms: 42,
            server_tick: 4004,
        });
        rt_server(ServerMsg::CaptainView {
            tick: 4000,
            full: true,
            upserts: vec![MapUnit {
                entity: 3,
                kind: EntityKind::PoliceChase,
                x_dm: -1200,
                y_dm: 5000,
                heading: 64,
                wanted: 0,
                order: 3,
                unit: Some(PLAYER_UNIT_BASE + 3),
            }],
            removed: vec![8],
            incidents: vec![Incident {
                x_dm: 10,
                y_dm: 20,
                kind: 0,
                age_s: 4,
            }],
            heat: vec![(1, -2, 200)],
        });
        rt_server(ServerMsg::UnitOrder(UnitOrder::Intercept {
            target: EntityId(5),
        }));
        rt_server(ServerMsg::UnitOrder(UnitOrder::Disengage));
        rt_server(ServerMsg::RaceUpdate(RaceInfo {
            race: 1,
            host: 3,
            state: RaceState::Countdown,
            start_tick: 9000,
            stake: 750,
            laps: 1,
            checkpoints: vec![[0, 0], [500, 500]],
            members: vec![3, 4],
        }));
        rt_server(ServerMsg::RaceProgress {
            race: 1,
            player: 3,
            next_checkpoint: 2,
        });
        rt_server(ServerMsg::RaceResults {
            race: 1,
            entries: vec![RaceResultEntry {
                player: 3,
                place: 1,
                time_ticks: 12000,
                payout: 900,
            }],
        });
        rt_server(ServerMsg::RaceInvited { race: 1, from: 3 });
        rt_server(ServerMsg::Account {
            cash: -50,
            rep: 10,
            cop_rank: 7,
            bounty: 0,
        });
        rt_server(ServerMsg::Notice { text: "hi".into() });
        rt_server(ServerMsg::Events {
            tick: 4004,
            events: sample_events(),
        });
        rt_server(ServerMsg::CaptainReply {
            seq: 7,
            result: Ok(()),
        });
        rt_server(ServerMsg::CaptainReply {
            seq: 8,
            result: Err(CaptainError::Cooldown { remaining_s: 4.5 }),
        });
        rt_server(ServerMsg::CaptainReply {
            seq: 9,
            result: Err(CaptainError::Site(wp_contracts::SiteError::BlindCorner)),
        });
        let mut water_q8 = vec![0u8; 500];
        water_q8[100..140].fill(9);
        water_q8[499] = 200;
        rt_server(ServerMsg::Weather(WeatherSnapshot {
            kind: WeatherKind::HeavyRain,
            intensity: 0.75,
            prev_kind: WeatherKind::Clear,
            prev_intensity: 0.0,
            blend: 0.4,
            wind_heading_rad: -1.25,
            water_q8,
        }));
        rt_server(ServerMsg::Weather(WeatherSnapshot {
            kind: WeatherKind::Clear,
            intensity: 0.0,
            prev_kind: WeatherKind::Clear,
            prev_intensity: 0.0,
            blend: 1.0,
            wind_heading_rad: 0.0,
            water_q8: vec![],
        }));
    }

    /// One typed event of every kind the server forwards.
    fn sample_events() -> Vec<SimEvent> {
        use wp_contracts::*;
        vec![
            SimEvent::Police(PoliceEvent::Radio {
                unit: Some(UnitId(4)),
                text: "Unit 4 in pursuit, heading north.".into(),
            }),
            SimEvent::Police(PoliceEvent::PursuitEnded {
                racer: EntityId(1),
                outcome: PursuitOutcome::Captured,
                level: WantedLevel::L3,
                capturer: Some(UnitId(4)),
                assists: vec![UnitId(5), UnitId(9)],
            }),
            SimEvent::Police(PoliceEvent::SpikesPlaced {
                tool: ToolId(2),
                racer: EntityId(1),
                a: [1.0, 2.0, 0.0],
                b: [5.0, 2.0, 0.0],
                sight_m: 80.5,
            }),
            SimEvent::Race(RaceEvent::Countdown {
                event: RaceEventId(3),
                green_tick: 99_000,
            }),
            SimEvent::Race(RaceEvent::RacerDnf {
                event: RaceEventId(3),
                racer: EntityId(1),
                reason: DnfReason::Fled,
            }),
            SimEvent::Economy(EconomyEvent {
                account: AccountId(1),
                seq: 4,
                day: 2,
                reason: EconomyReason::RacePayout {
                    race: RaceType::Circuit,
                    placement: 1,
                },
                car: None,
                cash_delta: 1_400,
                rep_delta: 10,
                bounty_delta: 0,
                cop_xp_delta: 0,
                cash_after: 2_000,
            }),
            SimEvent::WeatherChanged {
                kind: WeatherKind::LightRain,
                intensity: 0.5,
                forced: false,
            },
        ]
    }

    #[test]
    fn garbage_is_rejected() {
        assert!(ClientMsg::decode(&[]).is_err());
        assert!(ClientMsg::decode(&[0xFF]).is_err());
        assert!(ServerMsg::decode(&[0x01]).is_err());
        // Trailing bytes.
        let mut b = ClientMsg::Bye.encode();
        b.push(0);
        assert!(ClientMsg::decode(&b).is_err());
        // Oversized captain command.
        let mut w = ByteWriter::new();
        w.u8(0x04);
        w.varint(1);
        w.bytes(&[0u8; MAX_COMMAND_LEN + 1]);
        assert!(ClientMsg::decode(&w.into_bytes()).is_err());
        // A typed payload with an unknown variant, or with trailing bytes inside its length.
        let mut w = ByteWriter::new();
        w.u8(0x0D);
        w.bytes(&[0x7F]);
        assert!(ClientMsg::decode(&w.into_bytes()).is_err());
        let mut w = ByteWriter::new();
        w.u8(0x0D);
        w.bytes(&[2, 0]); // `Ready` + one stray byte
        assert!(ClientMsg::decode(&w.into_bytes()).is_err());
        // Retired v3 lobby tags are unknown now.
        assert!(ClientMsg::decode(&[0x08, 1]).is_err());
        // A weather grid whose runs overflow the cell count.
        let mut w = ByteWriter::new();
        w.u8(0x96);
        w.u8(0);
        w.f32(0.0);
        w.u8(0);
        w.f32(0.0);
        w.f32(1.0);
        w.f32(0.0);
        w.varint(4);
        w.varint(1);
        w.varint(5);
        w.u8(1);
        assert!(ServerMsg::decode(&w.into_bytes()).is_err());
    }
}
