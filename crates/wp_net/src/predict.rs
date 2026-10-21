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
use wp_vehicle::VehicleSnapshot;

/// Static city ground for prediction rigs: the parked safety slab plus the city's terrain,
/// road, building and rail colliders, exactly as the server's `wp_sim` builds them (see
/// [`crate::netsim::city_ground`]). Shared between rigs; colliders clone cheaply (shapes are
/// reference counted).
pub type CityGround = std::sync::Arc<Vec<rapier3d::prelude::Collider>>;

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
    /// Rewind to a server state (approximate: `VehicleState` omits filters, clutch, …).
    fn set_state(&mut self, s: &VehicleState);
    /// Rewind bit-exactly to a server `wp_vehicle::VehicleSnapshot` (bytes). Returns false if
    /// the snapshot cannot be used (wrong car, bad bytes); the caller falls back to
    /// [`PredictedVehicle::set_state`].
    fn restore(&mut self, snapshot: &[u8]) -> bool;
}

/// [`PredictedVehicle`] over the real `wp_vehicle` model: the car's own Rapier chassis body on
/// a flat ground slab, probed with `RapierGround` and stepped exactly like `wp_sim` steps it
/// (`wp_vehicle::rig::Rig`). Rewind sets both the vehicle and the rigid body.
///
/// Differences from the server that reconciliation absorbs: no other cars (collisions are not
/// predicted) and no surface map (asphalt everywhere). In cities with terrain the rig drives on
/// the server's own static ground ([`CityGround`]); otherwise on the flat slab.
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
        Self::on_ground(car, assists, state, time, None)
    }

    /// [`RigVehicle::new`] on a city's static ground (`None` = flat slab).
    pub fn on_ground(
        car: &LoadedCar,
        assists: AssistProfile,
        state: &VehicleState,
        time: TimeOfDay,
        ground: Option<&CityGround>,
    ) -> Self {
        let mut rig = Rig::new(car);
        // The client predicts its own player's car: the server's sim gives that car the
        // player steering aids (`wp_sim`, `VehicleKind::Player`), so must the prediction.
        rig.set_player_input(true);
        if let Some(g) = ground {
            rig.set_city_ground(g.iter().cloned());
        }
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

    fn restore(&mut self, snapshot: &[u8]) -> bool {
        self.rig
            .restore(&VehicleSnapshot::from_bytes(snapshot.to_vec()))
            .is_ok()
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

    /// [`Predictor::new`] on a city's static ground (`None` = flat slab).
    pub fn on_ground(
        car: &LoadedCar,
        assists: AssistProfile,
        state: &VehicleState,
        tick: u64,
        time: TimeOfDay,
        ground: Option<&CityGround>,
    ) -> Self {
        let v = RigVehicle::on_ground(car, assists, state, time, ground);
        Self::with_vehicle(Box::new(v), tick)
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

    /// Overwrite the current state with an exact server snapshot (no tick change).
    pub fn restore_exact(&mut self, snapshot: &[u8]) -> bool {
        self.vehicle.restore(snapshot)
    }

    /// Compare with the server's state at `server_tick` and correct if needed. With `exact`
    /// (the server's bit-exact vehicle snapshot for `server_tick`) the rewind is lossless;
    /// otherwise it falls back to [`PredictedVehicle::set_state`].
    pub fn reconcile(
        &mut self,
        server_tick: u64,
        server: &VehicleState,
        exact: Option<&[u8]>,
        effects: &HandlingEffects,
    ) -> Reconcile {
        if server_tick > self.tick {
            self.rewind_to(server, exact);
            self.tick = server_tick;
            return Reconcile::Reset;
        }
        if server_tick == 0 {
            return Reconcile::Stale;
        }
        let Some(pred) = self.frame(server_tick - 1).copied() else {
            if self.tick - server_tick >= INPUT_RING as u64 {
                // We are more than a second ahead of the server: give up on history.
                self.rewind_to(server, exact);
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
        self.rewind_to(server, exact);
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

    fn rewind_to(&mut self, server: &VehicleState, exact: Option<&[u8]>) {
        if !exact.is_some_and(|b| self.vehicle.restore(b)) {
            self.vehicle.set_state(server);
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
        let mut rig = Rig::new(&car());
        rig.set_player_input(true);
        rig.state()
    }

    /// Client predicts 2 s in lock-step with a server `Rig` (exactly how `wp_sim` steps the
    /// car), then the server applies different input for 0.5 s. Returns the client after its
    /// 130 diverged ticks, the server rig at tick 600 and the client's inputs.
    fn diverge() -> (Predictor, Rig, AssistProfile) {
        let c = car();
        let fx = HandlingEffects::NEUTRAL;
        let assists = AssistProfile::default();
        let noon = TimeOfDay::from_hours(12.0);
        let mut server = Rig::new(&c);
        // The server's sim car is the player's: the player steering aids, as the prediction.
        server.set_player_input(true);
        let mut client = Predictor::new(&c, assists, &server.state(), 0, noon);
        // As on join: the first own-state carries the exact snapshot.
        assert!(client.restore_exact(server.snapshot().as_bytes()));
        let throttle = quantize_input(&VehicleInput {
            throttle: 1.0,
            steer: 0.2,
            ..Default::default()
        });
        for _ in 0..480 {
            client.step(throttle, &fx);
            server.step(&throttle, &assists);
        }
        assert!(matches!(
            client.reconcile(480, &server.state(), None, &fx),
            Reconcile::InSync { error_m } if error_m == 0.0
        ));
        let other = quantize_input(&VehicleInput {
            steer: -1.0,
            brake: 1.0,
            ..Default::default()
        });
        for _ in 0..120 {
            server.step(&other, &assists);
        }
        for _ in 0..130 {
            client.step(throttle, &fx);
        }
        (client, server, assists)
    }

    fn replay_gap(
        mut client: Predictor,
        mut server: Rig,
        assists: AssistProfile,
        exact: bool,
    ) -> f32 {
        let fx = HandlingEffects::NEUTRAL;
        let snap = server.snapshot();
        let bytes = exact.then(|| snap.as_bytes().to_vec());
        let r = client.reconcile(600, &server.state(), bytes.as_deref(), &fx);
        assert!(matches!(r, Reconcile::Corrected { .. }), "{r:?}");
        assert_eq!(client.tick(), 610);
        // The server then receives the client's inputs for the replayed ticks.
        for t in 600..610 {
            server.step(&client.input_at(t).unwrap(), &assists);
        }
        dist(server.state().pos, client.state().pos)
    }

    #[test]
    fn exact_snapshot_rewind_replays_bit_for_bit() {
        let (client, server, assists) = diverge();
        let d = replay_gap(client, server, assists, true);
        assert_eq!(
            d, 0.0,
            "snapshot rewind + replay must reproduce the server exactly"
        );
    }

    #[test]
    fn state_rewind_fallback_is_close() {
        let (client, server, assists) = diverge();
        let d = replay_gap(client, server, assists, false);
        // `VehicleState` omits filters, clutch and tire relaxation: ≈1 cm after 10 ticks.
        assert!(d > 0.0 && d < 0.05, "{d}");
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
            p.reconcile(500, &start(), None, &HandlingEffects::NEUTRAL),
            Reconcile::Reset
        );
        assert_eq!(p.tick(), 500);
    }
}
