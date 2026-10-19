//! Harness-side net round trip on workstream F's real wire codec (`wp_net`): every net tick the
//! vehicle states are quantised (`QVehicle`), delta-encoded against the previous snapshot like
//! the server does (`DeltaVehicle`, absolute every [`ABSOLUTE_EVERY`] net ticks), wrapped in a
//! `ServerMsg::Snapshot` with the first vehicle as the client's own (`OwnState`), encoded to
//! bytes, decoded, and the deltas re-applied the way a client would. Any mismatch is a failure.
//! It also tracks the worst dequantised position error against the true state.

use std::collections::BTreeMap;

use wp_contracts::{EntityId, VehicleState};
use wp_net::protocol::ServerMsg;
use wp_net::quant::{dq_pos, QVehicle};
use wp_net::snapshot::{DeltaVehicle, OwnState};

/// Send every vehicle absolutely (no baseline) once per this many net ticks, as a resync.
pub const ABSOLUTE_EVERY: u64 = 60;

/// Worst allowed dequantised position error, m (the codec quantises to 1 cm).
pub const MAX_POS_ERR_M: f32 = 0.0051;

/// Sender + receiver state for the round trip.
#[derive(Debug, Default)]
pub struct NetRoundTrip {
    net_ticks: u64,
    /// Sender: last quantised state sent per entity, with its tick.
    sent: BTreeMap<u32, (u64, QVehicle)>,
    /// Receiver: reconstructed state per entity.
    received: BTreeMap<u32, QVehicle>,
    /// Snapshots round-tripped.
    pub snapshots: u64,
    /// Total encoded bytes.
    pub bytes: u64,
    /// Entities sent as deltas (not absolute).
    pub deltas: u64,
    /// Worst dequantised position error seen, m.
    pub max_pos_err_m: f32,
}

/// Build the server's snapshot message for `states` at `tick`, delta-encoding against
/// `sent` (updated in place). `absolute` forces every entity to be sent without a baseline.
pub fn build_snapshot(
    tick: u64,
    states: &[(EntityId, VehicleState)],
    sent: &mut BTreeMap<u32, (u64, QVehicle)>,
    absolute: bool,
) -> ServerMsg {
    let mut entities = Vec::with_capacity(states.len());
    for (id, s) in states {
        let q = QVehicle::from_state(s);
        let base = sent
            .get(&id.0)
            .filter(|_| !absolute)
            .map(|(t, b)| ((tick - t) as u32, b));
        entities.push(DeltaVehicle::encode(id.0, &q, base));
        sent.insert(id.0, (tick, q));
    }
    ServerMsg::Snapshot {
        tick,
        input_ack: 0,
        own: states.first().map(|(_, s)| OwnState::from_state(s)),
        own_full: None,
        entities,
        traffic: None,
    }
}

impl NetRoundTrip {
    /// Round-trip one snapshot of `states` at `tick`. `Err` describes the first mismatch.
    pub fn snapshot(
        &mut self,
        tick: u64,
        states: &[(EntityId, VehicleState)],
    ) -> Result<(), String> {
        let absolute = self.net_ticks.is_multiple_of(ABSOLUTE_EVERY);
        self.net_ticks += 1;
        let msg = build_snapshot(tick, states, &mut self.sent, absolute);
        let bytes = msg.encode();
        self.snapshots += 1;
        self.bytes += bytes.len() as u64;
        let back = ServerMsg::decode(&bytes).map_err(|e| format!("decode: {e}"))?;
        // Field-wise equality is too strict: `DeltaVehicle` keeps `rot`/`misc` values its mask
        // omits, which the decoder leaves at their defaults. Compare the wire form instead.
        if back.encode() != bytes {
            return Err("decoded message re-encodes differently".into());
        }
        let ServerMsg::Snapshot { entities, own, .. } = back else {
            return Err("decoded a different message type".into());
        };
        if own != states.first().map(|(_, s)| OwnState::from_state(s)) {
            return Err("own vehicle state did not round-trip exactly".into());
        }
        for (d, (id, truth)) in entities.iter().zip(states) {
            if d.baseline_age > 0 {
                self.deltas += 1;
            }
            let q = d
                .apply(self.received.get(&d.entity))
                .map_err(|e| format!("entity {}: {e}", d.entity))?;
            if Some(&q) != self.sent.get(&id.0).map(|(_, s)| s) {
                return Err(format!("entity {}: delta reconstruction differs", d.entity));
            }
            for k in 0..3 {
                let err = (dq_pos(q.pos[k]) - truth.pos[k]).abs();
                self.max_pos_err_m = self.max_pos_err_m.max(err);
            }
            self.received.insert(d.entity, q);
        }
        if self.max_pos_err_m > MAX_POS_ERR_M {
            return Err(format!(
                "position quantisation error {} m > {MAX_POS_ERR_M} m",
                self.max_pos_err_m
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wp_contracts::VehicleState;

    fn state(x: f32, yaw_q: [f32; 4]) -> VehicleState {
        VehicleState {
            pos: [x, 2.0, 0.5],
            rot: yaw_q,
            lin_vel: [1.0, 3.0, 0.0],
            ..Default::default()
        }
    }

    #[test]
    fn round_trips_moving_vehicles() {
        let mut rt = NetRoundTrip::default();
        let q = [0.0, 0.0, 0.0, 1.0];
        for i in 0..200u64 {
            let x = i as f32 * 0.37;
            let states = vec![(EntityId(1), state(x, q)), (EntityId(2), state(-x, q))];
            rt.snapshot(4 * (i + 1), &states)
                .unwrap_or_else(|e| panic!("net tick {i}: {e}"));
        }
        assert!(rt.deltas > 0);
        assert!(rt.max_pos_err_m <= MAX_POS_ERR_M);
    }
}
