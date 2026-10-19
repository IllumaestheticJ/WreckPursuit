//! Logical network messages. Since v0.3.0 (NET-1) `wp_net::protocol` is the source of truth
//! for the **wire** (handshake, acks, time sync, quantised snapshots, lattice resync, lobby);
//! these are the logical messages other crates may produce or consume, with typed payloads.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    CaptainCommand, CaptainError, EntityId, PlayerId, RaceCommand, Role, SimEvent, VehicleInput,
    VehicleState, WeatherKind,
};

/// One kinematic traffic agent in a client's interest set (v0.3.0, world #6). Physical
/// (promoted) agents replicate as ordinary vehicles; despawns are implicit (absent id).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TrafficAgentSnapshot {
    /// `wp_world` agent id, stable for the agent's life.
    pub id: u32,
    /// Position, world m (`wp_net` may quantise on the wire).
    pub pos: [f32; 3],
    /// Heading, rad (0 = +Y).
    pub yaw: f32,
    /// Speed along the heading, m/s (client-side extrapolation).
    pub speed_mps: f32,
    /// Bit 0 brake lights, 1 hazards, 2 left signal, 3 right signal, 4 headlights,
    /// 5 yielding to a siren.
    pub flags: u8,
    /// Visual variant (car model / colour).
    pub variant: u8,
}

/// Bits of [`TrafficAgentSnapshot::flags`].
pub mod traffic_flags {
    /// Brake lights on.
    pub const BRAKE: u8 = 1;
    /// Hazard lights on.
    pub const HAZARDS: u8 = 1 << 1;
    /// Left turn signal.
    pub const LEFT_SIGNAL: u8 = 1 << 2;
    /// Right turn signal.
    pub const RIGHT_SIGNAL: u8 = 1 << 3;
    /// Headlights on.
    pub const HEADLIGHTS: u8 = 1 << 4;
    /// Yielding to a siren.
    pub const YIELDING: u8 = 1 << 5;
}

/// Low-rate weather state for clients (v0.3.0, weather W-5).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct WeatherSnapshot {
    /// Current regime.
    pub kind: WeatherKind,
    /// Current intensity 0..1.
    pub intensity: f32,
    /// Regime being blended from.
    pub prev_kind: WeatherKind,
    /// Intensity being blended from.
    pub prev_intensity: f32,
    /// Blend 0 (prev) ..= 1 (current).
    pub blend: f32,
    /// Wind heading, rad.
    pub wind_heading_rad: f32,
    /// Road water grid, quantised (`wp_weather` grid order, 1/8 mm per step).
    pub water_q8: Vec<u8>,
}

/// Messages from a client to the authoritative server.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub enum ClientToServer {
    /// Input for the client's own vehicle at a given physics tick.
    Input {
        /// Physics tick the input applies to.
        tick: u64,
        /// The input.
        input: VehicleInput,
    },
    /// Ask for a role.
    RoleRequest {
        /// Requested role.
        role: Role,
    },
    /// Police captain command (v0.3.0: typed, NET-3).
    CaptainCommand(CaptainCommand),
    /// Race lobby command (v0.3.0, race R6).
    RaceCommand(RaceCommand),
}

/// Messages from the server to a client.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub enum ServerToClient {
    /// Per-tick (60 Hz) snapshot.
    Snapshot {
        /// Physics tick of the snapshot.
        tick: u64,
        /// Visible vehicles, ascending by entity id.
        vehicles: Vec<(EntityId, VehicleState)>,
        /// Kinematic traffic in the client's interest set, ascending id (v0.3.0).
        #[serde(default)]
        traffic: Vec<TrafficAgentSnapshot>,
        /// Events since the last snapshot.
        events: Vec<SimEvent>,
    },
    /// Full baseline on join / after desync; encoding defined by `wp_net` (opaque here).
    Baseline {
        /// Physics tick of the baseline.
        tick: u64,
        /// Encoded baseline.
        payload: Vec<u8>,
    },
    /// The server assigned a role.
    RoleAssigned {
        /// The player.
        player: PlayerId,
        /// Assigned role.
        role: Role,
        /// Vehicle controlled by the player, if any.
        vehicle: Option<EntityId>,
    },
    /// Reply to a [`ClientToServer::CaptainCommand`] (v0.3.0).
    CaptainReply(Result<(), CaptainError>),
    /// Weather state (v0.3.0, W-5; sent on join and on change, low rate).
    Weather(WeatherSnapshot),
}
