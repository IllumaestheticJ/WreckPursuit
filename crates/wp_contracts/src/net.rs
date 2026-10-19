//! Network message skeletons. `wp_net` owns encoding, quantisation and delta compression;
//! these are the logical messages.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{EntityId, PlayerId, Role, SimEvent, VehicleInput, VehicleState};

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
    /// Police captain command; payload defined by `wp_police` (opaque here).
    CaptainCommand(Vec<u8>),
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
}
