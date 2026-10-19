//! Client-side prediction of the local vehicle with rewind-and-replay reconciliation.
//!
//! The client runs the same `wp_vehicle` code as the server, one physics tick per local input.
//! It keeps the last [`INPUT_RING`] ticks of **inputs** (plus the predicted pose after each,
//! only to measure error). When the server's full-precision state for tick `T` arrives it is
//! compared with the prediction for `T`; beyond tolerance the vehicle is set to the server
//! state and every stored input from `T` on is re-simulated.

use wp_contracts::{
    AssistProfile, HandlingEffects, LoadedCar, TimeOfDay, VehicleInput, VehicleState,
};
use wp_vehicle::rig::Rig;

/// Inputs kept for replay: 1 s at 240 Hz.
pub const INPUT_RING: usize = 240;
/// Position error that triggers a correction, m.
pub const POS_TOLERANCE_M: f32 = 0.05;
/// Rotation error that triggers a correction, rad.
pub const ROT_TOLERANCE_RAD: f32 = 0.02;

#[derive(Debug, Clone, Copy)]
struct Frame {
    tick: u64,
    input: VehicleInput,
    pos_after: [f32; 3],
    rot_after: [f32; 4],
}

/// Outcome of [`Predictor::reconcile`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Reconcile {
    /// Prediction matched within tolerance.
    InSync {
        /// Position error at the server tick, m.
        error_m: f32,
    },
    /// Rewound and replayed.
    Corrected {
        /// Position error at the server tick, m.
        error_m: f32,
        /// How far the *current* predicted position moved because of the replay, m.
        correction_m: f32,
    },
    /// No usable history (too old, or the server is ahead): snapped to the server state.
    Reset,
    /// Server state older than anything we could compare; ignored.
    Stale,
}

/// The vehicle model a [`Predictor`] drives. It must run exactly the code the server runs
/// for one physics tick, and must be rewindable to a server state. [`RigVehicle`] is the
/// production impl.
pub trait PredictedVehicle: Send {
    /// One physics tick with `input` under `effects`.
    fn step(&mut self, input: &VehicleInput, effects: &HandlingEffects);
    /// Current state.
    fn state(&self) -> VehicleState;
    /// Rewind to a server state.
    fn set_state(&mut self, s: &VehicleState);
}

/// [`PredictedVehicle`] over the real `wp_vehicle` model: the car's own Rapier chassis body on
/// a flat ground slab, probed with `RapierGround` and stepped exactly like `wp_sim` steps it
/// (`wp_vehicle::rig::Rig`). Rewind sets both the vehicle and the rigid body.
///
/// Differences from the server that reconciliation absorbs: the client has no city geometry
/// or other cars (collisions are not predicted) and no surface map (asphalt everywhere).
pub struct RigVehicle {
    rig: Rig,
    assists: AssistProfile,
}

impl RigVehicle {
    /// Build `car` at `state`.
    pub fn new(
        car: &LoadedCar,
        assists: AssistProfile,
        state: &VehicleState,
        time: TimeOfDay,
    ) -> Self {
        let mut rig = Rig::new(car);
        rig.world.time = time;
        rig.set_state(state);
        Self { rig, assists }
    }
}

impl PredictedVehicle for RigVehicle {
    fn step(&mut self, input: &VehicleInput, effects: &HandlingEffects) {
        self.rig.set_effects(effects);
        self.rig.step(input, &self.assists);
        // Events (FFB, surface) are the server's business; do not let them pile up.
        self.rig.events.clear();
    }

    fn state(&self) -> VehicleState {
        self.rig.state()
    }

    fn set_state(&mut self, s: &VehicleState) {
        self.rig.set_state(s);
    }
}

/// Predicted local vehicle.
pub struct Predictor {
    vehicle: Box<dyn PredictedVehicle>,
    tick: u64,
    ring: Vec<Option<Frame>>,
}

fn quat_angle_exceeds(a: [f32; 4], b: [f32; 4], rad: f32) -> bool {
    let dot = (a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3]).abs();
    // angle = 2 acos(dot) > rad  ⇔  dot < cos(rad / 2); cos via its Taylor series is plenty.
    let half = rad * 0.5;
    dot < 1.0 - half * half * 0.5
}

fn dist(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

impl Predictor {
    /// Start predicting `car` from `state` (state *at* `tick`).
    pub fn new(
        car: &LoadedCar,
        assists: AssistProfile,
        state: &VehicleState,
        tick: u64,
        time: TimeOfDay,
    ) -> Self {
        Self::with_vehicle(Box::new(RigVehicle::new(car, assists, state, time)), tick)
    }

    /// Predict with any [`PredictedVehicle`] whose state is at `tick`.
    pub fn with_vehicle(vehicle: Box<dyn PredictedVehicle>, tick: u64) -> Self {
        Self {
            vehicle,
            tick,
            ring: vec![None; INPUT_RING],
        }
    }

    /// Tick the current predicted state is at.
    pub fn tick(&self) -> u64 {
        self.tick
    }

    /// Current predicted state.
    pub fn state(&self) -> VehicleState {
        self.vehicle.state()
    }

    /// Simulate one tick with `input` (must already be quantised, see
    /// [`crate::quant::quantize_input`]) under the given damage effects.
    pub fn step(&mut self, input: VehicleInput, effects: &HandlingEffects) {
        self.vehicle.step(&input, effects);
        let s = self.vehicle.state();
        self.ring[(self.tick % INPUT_RING as u64) as usize] = Some(Frame {
            tick: self.tick,
            input,
            pos_after: s.pos,
            rot_after: s.rot,
        });
        self.tick += 1;
    }

    /// Jump the tick counter without simulating (the client fell hopelessly behind or ahead of
    /// its target). The state is kept; the next server state corrects it.
    pub fn warp(&mut self, tick: u64) {
        self.tick = tick;
    }

    /// Stored input for `tick`, if still in the ring.
    pub fn input_at(&self, tick: u64) -> Option<VehicleInput> {
        self.frame(tick).map(|f| f.input)
    }

    fn frame(&self, tick: u64) -> Option<&Frame> {
        self.ring[(tick % INPUT_RING as u64) as usize]
            .as_ref()
            .filter(|f| f.tick == tick)
    }

    /// Compare with the server's state at `server_tick` and correct if needed.
    pub fn reconcile(
        &mut self,
        server_tick: u64,
        server: &VehicleState,
        effects: &HandlingEffects,
    ) -> Reconcile {
        if server_tick > self.tick {
            self.vehicle.set_state(server);
            self.tick = server_tick;
            return Reconcile::Reset;
        }
        if server_tick == 0 {
            return Reconcile::Stale;
        }
        let Some(pred) = self.frame(server_tick - 1).copied() else {
            if self.tick - server_tick >= INPUT_RING as u64 {
                // We are more than a second ahead of the server: give up on history.
                self.vehicle.set_state(server);
                self.tick = server_tick;
                return Reconcile::Reset;
            }
            return Reconcile::Stale;
        };
        let error_m = dist(pred.pos_after, server.pos);
        if error_m <= POS_TOLERANCE_M
            && !quat_angle_exceeds(pred.rot_after, server.rot, ROT_TOLERANCE_RAD)
        {
            return Reconcile::InSync { error_m };
        }
        let before = self.vehicle.state().pos;
        let now = self.tick;
        self.vehicle.set_state(server);
        self.tick = server_tick;
        for t in server_tick..now {
            let input = self.input_at(t).unwrap_or_default();
            self.step(input, effects);
        }
        Reconcile::Corrected {
            error_m,
            correction_m: dist(before, self.vehicle.state().pos),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quant::quantize_input;

    fn car() -> LoadedCar {
        let p = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/cars/debug_sedan.json"
        );
        wp_contracts::load_car(p).unwrap()
    }

    fn start() -> VehicleState {
        Rig::new(&car()).state()
    }

    #[test]
    fn identical_inputs_stay_in_sync_and_divergence_is_replayed() {
        let c = car();
        let fx = HandlingEffects::NEUTRAL;
        let assists = AssistProfile::default();
        let noon = TimeOfDay::from_hours(12.0);
        let mut client = Predictor::new(&c, assists, &start(), 0, noon);
        let mut server = Predictor::new(&c, assists, &start(), 0, noon);
        let throttle = quantize_input(&VehicleInput {
            throttle: 1.0,
            steer: 0.2,
            ..Default::default()
        });
        // 2 s of identical driving: bit-identical.
        for _ in 0..480 {
            client.step(throttle, &fx);
            server.step(throttle, &fx);
        }
        assert!(matches!(
            client.reconcile(480, &server.state(), &fx),
            Reconcile::InSync { error_m } if error_m == 0.0
        ));
        // The server applied different input for 0.5 s (e.g. lost packets) → correction.
        let other = quantize_input(&VehicleInput {
            steer: -1.0,
            brake: 1.0,
            ..Default::default()
        });
        for _ in 0..120 {
            server.step(other, &fx);
        }
        for _ in 0..130 {
            client.step(throttle, &fx);
        }
        let r = client.reconcile(600, &server.state(), &fx);
        assert!(matches!(r, Reconcile::Corrected { .. }), "{r:?}");
        assert_eq!(client.tick(), 610);
        // After replay the client agrees with a server that then receives the client's inputs.
        for t in 600..610 {
            server.step(client.input_at(t).unwrap(), &fx);
        }
        let d = dist(server.state().pos, client.state().pos);
        // Not bit-exact: `VehicleState` omits input filters, clutch and tire relaxation, so a
        // rewind leaves ≈1 cm after 10 ticks (measured 0.014 m). Lossless needs NET-N2.
        assert!(d < 0.05, "{d}");
    }

    #[test]
    fn far_behind_server_resets() {
        let c = car();
        let mut p = Predictor::new(
            &c,
            AssistProfile::default(),
            &start(),
            0,
            TimeOfDay::default(),
        );
        assert_eq!(
            p.reconcile(500, &start(), &HandlingEffects::NEUTRAL),
            Reconcile::Reset
        );
        assert_eq!(p.tick(), 500);
    }
}
