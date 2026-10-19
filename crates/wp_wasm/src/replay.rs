//! Replay format. A replay is **not** a state recording: it stores the session header (seed,
//! damage mode, clock), the complete tick-stamped op log (spawns, input changes, resets,
//! assist changes) from tick 0, and a state hash every [`HASH_INTERVAL_TICKS`] ticks.
//! Playback is deterministic re-simulation; the stored hashes detect drift.
//!
//! Encoding: `postcard` of [`Replay`], prefixed with the 4-byte magic [`MAGIC`].

use serde::{Deserialize, Serialize};
use wp_contracts::{AssistProfile, DamageMode, VehicleInput};

/// File magic: `WPR` + format version byte.
pub const MAGIC: [u8; 4] = *b"WPR\x01";

/// A state hash is stored every this many ticks (0.25 s at 240 Hz).
pub const HASH_INTERVAL_TICKS: u64 = 60;

/// Everything needed to rebuild the initial (empty) simulation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionHeader {
    /// `wp_contracts::CONTRACT_VERSION` the replay was recorded with.
    pub contract_version: String,
    /// Master RNG seed.
    pub seed: u64,
    /// Damage mode.
    pub mode: DamageMode,
    /// Start time of day, hours.
    pub time_of_day_h: f32,
    /// In-game seconds per simulated second.
    pub time_ratio: f32,
    /// Scenario name (informational).
    pub scenario: String,
}

/// One operation applied to the simulation before the tick it is stamped with.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Op {
    /// Spawn a car. `entity` is the id the sim allocated (checked on playback).
    Spawn {
        /// Allocated entity id.
        entity: u32,
        /// Car id.
        car_id: String,
        /// World position, m.
        pos: [f32; 3],
        /// Heading, rad.
        yaw: f32,
    },
    /// New input for a vehicle (logged only when it changes).
    Input {
        /// Vehicle.
        entity: u32,
        /// Input held from this tick on.
        input: VehicleInput,
    },
    /// Reset a vehicle to its spawn pose.
    Reset {
        /// Vehicle.
        entity: u32,
    },
    /// Change a vehicle's assists.
    Assists {
        /// Vehicle.
        entity: u32,
        /// New profile.
        profile: AssistProfile,
    },
}

/// An op stamped with the tick it applies before (i.e. `Sim::tick()` at the time it was issued).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TickOp {
    /// Tick the op applies before.
    pub tick: u64,
    /// The op.
    pub op: Op,
}

/// A complete replay.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Replay {
    /// Initial conditions.
    pub header: SessionHeader,
    /// All ops from tick 0, ascending by tick (stable within a tick).
    pub ops: Vec<TickOp>,
    /// `(tick, Sim::state_hash())` every [`HASH_INTERVAL_TICKS`] plus the final tick.
    pub hashes: Vec<(u64, u64)>,
    /// Tick `record_start()` was called; playback starts here (earlier ticks are re-simulated
    /// silently to reproduce the state).
    pub view_start_tick: u64,
    /// Last recorded tick.
    pub end_tick: u64,
}

impl Replay {
    /// Encode as bytes (`MAGIC` + postcard).
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = MAGIC.to_vec();
        out.extend(postcard::to_allocvec(self).expect("replay serializes"));
        out
    }

    /// Decode bytes produced by [`Replay::to_bytes`].
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        let body = bytes
            .strip_prefix(&MAGIC)
            .ok_or("not a WreckPursuit replay (bad magic / version)")?;
        postcard::from_bytes(body).map_err(|e| format!("corrupt replay: {e}"))
    }

    /// Final recorded state hash.
    pub fn final_hash(&self) -> Option<u64> {
        self.hashes.last().map(|h| h.1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_round_trip() {
        let r = Replay {
            header: SessionHeader {
                contract_version: "0.1.0".into(),
                seed: 3,
                mode: DamageMode::Simulation,
                time_of_day_h: 21.5,
                time_ratio: 10.0,
                scenario: "x".into(),
            },
            ops: vec![TickOp {
                tick: 4,
                op: Op::Input {
                    entity: 1,
                    input: VehicleInput {
                        throttle: 0.5,
                        gear_up: true,
                        ..Default::default()
                    },
                },
            }],
            hashes: vec![(60, 0xdead_beef)],
            view_start_tick: 0,
            end_tick: 60,
        };
        assert_eq!(Replay::from_bytes(&r.to_bytes()).unwrap(), r);
        assert!(Replay::from_bytes(b"nope").is_err());
    }
}
