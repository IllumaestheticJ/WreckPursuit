//! # wp_net
//!
//! Multiplayer networking (owned by workstream 07): an authoritative server instance, client
//! prediction/reconciliation, damage replication by contact events, interest management,
//! roles, races, and the binary protocol. See `README.md` for the protocol spec.
//!
//! Everything except [`native`] is *sans-IO*: state machines that consume received bytes and
//! a caller-supplied clock (server ticks, client milliseconds) and produce bytes to send. No
//! wall-clock reads, no threads, no sockets — so the same code runs natively, in WASM and in
//! deterministic in-process tests over [`sim_link`].
//!
//! | module | what |
//! |---|---|
//! | [`wire`] | varint/zigzag byte reader + writer |
//! | [`quant`] | quantisation (1 cm positions, smallest-three rotations, 16-bit velocities, inputs, contacts) |
//! | [`snapshot`] | per-entity delta compression, full-precision own-car state |
//! | [`lattice`] | 32-bit lattice hash, lattice snapshots, [`lattice::ReplicaDamage`] |
//! | [`protocol`] | [`protocol::ClientMsg`] / [`protocol::ServerMsg`] and their codec |
//! | [`interest`] | interest categories, priority accumulators, byte budget |
//! | [`roles`] | role assignment, police ratio, captain seat hand-off |
//! | [`race`] | in-instance race lobby, checkpoint validation, payouts |
//! | [`server`] | [`server::ServerInstance`] over any [`server::NetSim`] |
//! | [`client`] | [`client::Client`]: prediction, interpolation, replicas, time sync |
//! | [`predict`] / [`interp`] / [`timesync`] | the client's building blocks |
//! | [`sim_link`] | in-memory netem-style transport (latency, jitter, loss) for tests |
//! | [`bot`] | scripted driver used by `wp-server --bots` and tests |
//! | [`testkit`] | crash arena sim + toy lattice that exercise damage replication |
//! | [`native`] | (feature `native`) tokio websocket server/client transports |

#![warn(missing_docs)]

pub mod bot;
pub mod client;
pub mod interest;
pub mod interp;
pub mod lattice;
pub mod netsim;
pub mod predict;
pub mod protocol;
pub mod quant;
pub mod race;
pub mod roles;
pub mod server;
pub mod sim_link;
pub mod snapshot;
pub mod testkit;
pub mod timesync;
pub mod wire;

#[cfg(feature = "native")]
pub mod native;

use thiserror::Error;

/// Delivery guarantee requested for a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Channel {
    /// Ordered, guaranteed delivery.
    Reliable,
    /// Best effort (may be reliable on transports that cannot do better, e.g. WebSocket).
    Unreliable,
}

/// Network errors.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum NetError {
    /// Message could not be decoded.
    #[error("decode error: {0}")]
    Decode(String),
    /// Transport failed or closed.
    #[error("transport error: {0}")]
    Transport(String),
}

/// A bidirectional message pipe carrying whole frames. WebSocket now (both channels map to the
/// same reliable stream); UDP/WebTransport later, where `Unreliable` becomes datagrams.
pub trait Transport {
    /// Queue `bytes` (one frame) for sending on `channel`.
    fn send(&mut self, channel: Channel, bytes: &[u8]) -> Result<(), NetError>;
    /// Return the next received frame, if any. Never blocks.
    fn poll(&mut self) -> Result<Option<Vec<u8>>, NetError>;
}

/// Physics ticks per second (240).
pub const TICK_HZ: u64 = 240;
/// Physics ticks per net tick (4).
pub const TICKS_PER_NET_TICK: u64 = wp_contracts::PHYSICS_TICKS_PER_NET_TICK as u64;
/// Down-link budget per client, bytes per second.
pub const DOWNLINK_BUDGET_BPS: usize = 40_000;
