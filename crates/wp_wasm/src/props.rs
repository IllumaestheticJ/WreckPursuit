//! City props for the renderer (lead helper, playtest row 50 "hitting invisible objects").
//!
//! Every `wp_world::World::static_props()` entry (parked cars, cones) is a Rapier body in the
//! sim ([`wp_sim::Sim::set_world`]): fixed until a vehicle comes near, then dynamic. The
//! browser draws them from two arrays:
//!
//! * [`static_list`] once per session: kind, pose and half extents of every prop, as the sim
//!   placed it (on hilly cities seated on the sloped road, not the raw world position);
//! * [`PropSync::moved`] every frame: only the props whose pose changed since the last call.
//!   Most props never move, so this is usually empty or a handful of records.
//!
//! Read-only: nothing here changes the simulation.

use std::collections::BTreeSet;

use wp_sim::Sim;
use wp_world::props::PropKind;

/// Floats per prop in [`static_list`]:
/// `[kind, x, y, z, qx, qy, qz, qw, half_x, half_y, half_z]` (box centre, world m;
/// half extents in the prop frame, +X right, +Y forward, +Z up).
pub const PROP_STRIDE: usize = 11;

/// Floats per record in [`PropSync::moved`]: `[prop index, x, y, z, qx, qy, qz, qw]`.
pub const MOVED_STRIDE: usize = 8;

/// Prop kind codes in [`static_list`].
pub const KIND_PARKED_CAR: f32 = 0.0;
/// See [`KIND_PARKED_CAR`].
pub const KIND_CONE: f32 = 1.0;
/// See [`KIND_PARKED_CAR`].
pub const KIND_KRAIL: f32 = 2.0;

/// Position change below which a prop counts as not moved, m.
const MOVE_EPS_M: f32 = 1e-4;
/// Quaternion component change below which a prop counts as not rotated.
const TURN_EPS: f32 = 1e-5;

/// Kind code of a prop.
pub fn kind_code(kind: PropKind) -> f32 {
    match kind {
        PropKind::ParkedCar => KIND_PARKED_CAR,
        PropKind::Cone => KIND_CONE,
        PropKind::KRail => KIND_KRAIL,
    }
}

fn pose7(sim: &Sim, i: usize) -> [f32; 7] {
    let (p, q) = sim.prop_pose(i).unwrap_or(([0.0; 3], [0.0, 0.0, 0.0, 1.0]));
    [p[0], p[1], p[2], q[0], q[1], q[2], q[3]]
}

/// Every prop of the session's city, [`PROP_STRIDE`] floats each, in sim order (the index
/// [`PropSync::moved`] records refer to). Empty without a city or with `parked_cars: false`.
pub fn static_list(sim: &Sim) -> Vec<f32> {
    let n = sim.prop_count();
    let mut out = Vec::with_capacity(n * PROP_STRIDE);
    // From the sim's own list: the city's props, then any a scenario added (`parked`).
    for i in 0..n {
        let kind = sim.prop_kind(i).unwrap_or(PropKind::Cone);
        out.push(kind_code(kind));
        out.extend_from_slice(&pose7(sim, i));
        out.extend_from_slice(&sim.prop_half_extents(i).unwrap_or([0.5; 3]));
    }
    out
}

/// Floats per record in [`wreck_list`].
pub const WRECK_STRIDE: usize = 12;

/// Dented parked cars ([`wp_sim::Sim::parked_wrecks`]), [`WRECK_STRIDE`] floats each:
/// `[prop index, awake, health, shattered glass regions, scale xyz, origin xyz, 0, 0]`. The
/// entity id is `wp_sim::PARKED_ENTITY_BASE + prop` (too large for an exact f32, so not in
/// the list).
pub fn wreck_list(sim: &Sim) -> Vec<f32> {
    let mut out = Vec::with_capacity(sim.parked_wrecks().len() * WRECK_STRIDE);
    for w in sim.parked_wrecks() {
        out.extend_from_slice(&[
            w.prop as f32,
            w.damage.is_awake() as u8 as f32,
            w.damage.health(),
            w.damage.shattered_regions().len() as f32,
        ]);
        out.extend_from_slice(&w.map.scale);
        out.extend_from_slice(&w.map.origin);
        out.extend_from_slice(&[0.0, 0.0]);
    }
    out
}

/// What the browser was last told about each prop, so a frame only carries the props that
/// moved.
#[derive(Debug, Default)]
pub struct PropSync {
    /// Last pose sent per prop (the static list counts as sent).
    sent: Vec<[f32; 7]>,
    /// Props that have moved at least once: checked every frame even after they turn fixed
    /// again, so a replay seek that puts them back is sent too. Small (knocked props only).
    touched: BTreeSet<usize>,
}

impl PropSync {
    /// Forget everything sent: the next [`PropSync::moved`] starts from the current poses
    /// (call when the static list is fetched, and for every new session).
    pub fn reset(&mut self, sim: &Sim) {
        self.sent = (0..sim.prop_count()).map(|i| pose7(sim, i)).collect();
        self.touched.clear();
    }

    /// Props whose pose changed since the last call (or the last [`PropSync::reset`]),
    /// [`MOVED_STRIDE`] floats each, ascending index. Checks only the props that are dynamic
    /// now or have moved before, so it costs nothing for the thousands that never move.
    pub fn moved(&mut self, sim: &Sim) -> Vec<f32> {
        if self.sent.len() != sim.prop_count() {
            self.reset(sim);
        }
        let candidates: BTreeSet<usize> = sim
            .active_props()
            .chain(self.touched.iter().copied())
            .collect();
        let mut out = Vec::new();
        for i in candidates {
            let now = pose7(sim, i);
            let was = self.sent[i];
            let moved = (0..3).any(|k| (now[k] - was[k]).abs() > MOVE_EPS_M)
                || (3..7).any(|k| (now[k] - was[k]).abs() > TURN_EPS);
            if !moved {
                continue;
            }
            self.sent[i] = now;
            self.touched.insert(i);
            out.push(i as f32);
            out.extend_from_slice(&now);
        }
        out
    }
}

/// Unit vector of the prop frame's +X axis in world XY for rotation `q` `[x, y, z, w]`.
pub fn prop_right_xy(q: [f32; 4]) -> [f32; 2] {
    let (x, y, z, w) = (q[0], q[1], q[2], q[3]);
    let rx = 1.0 - 2.0 * (y * y + z * z);
    let ry = 2.0 * (x * y + w * z);
    let l = (rx * rx + ry * ry).sqrt().max(1e-6);
    [rx / l, ry / l]
}

/// Parked cars (indices into [`static_list`]) nearest to `xy` first.
pub fn parked_cars_by_distance(sim: &Sim, xy: [f32; 2]) -> Vec<usize> {
    let n = sim.prop_count();
    let mut out: Vec<(f32, usize)> = (0..n)
        .filter(|i| sim.prop_kind(*i) == Some(PropKind::ParkedCar))
        .map(|i| {
            let p = pose7(sim, i);
            ((p[0] - xy[0]).powi(2) + (p[1] - xy[1]).powi(2), i)
        })
        .collect();
    out.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    out.into_iter().map(|(_, i)| i).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario;
    use crate::session::Session;
    use wp_contracts::DamageMode;

    /// A city free drive with the player's car only: no street racers (passing cars wake
    /// parked cars all over the city, which settle by under a centimetre, and these tests
    /// count the props near one car).
    fn city_session() -> Session {
        let sc = scenario::free_drive("debug_sedan", DamageMode::Arcade, true);
        crate::session::set_street_racers_enabled(false);
        let s = Session::new(sc, false);
        crate::session::set_street_racers_enabled(true);
        s.unwrap()
    }

    #[test]
    fn static_list_matches_the_sim_props() {
        let s = city_session();
        let list = static_list(s.sim());
        assert!(s.sim().prop_count() > 100, "city has parked cars");
        assert_eq!(list.len(), s.sim().prop_count() * PROP_STRIDE);
        for r in list.chunks(PROP_STRIDE) {
            assert!(r[0] == KIND_PARKED_CAR || r[0] == KIND_CONE || r[0] == KIND_KRAIL);
            let qn = r[4] * r[4] + r[5] * r[5] + r[6] * r[6] + r[7] * r[7];
            assert!((qn - 1.0).abs() < 1e-3, "unit quaternion");
            assert!(r[8] > 0.1 && r[9] > 0.1 && r[10] > 0.1, "half extents");
        }
    }

    #[test]
    fn only_props_that_moved_are_sent_and_they_follow_the_sim() {
        let mut s = city_session();
        let mut sync = PropSync::default();
        sync.reset(s.sim());
        let player = s.entities()[0];
        let target = s.debug_knock_prop(player, None, 12.0).unwrap();
        let mut seen = false;
        let mut drawn = pose7(s.sim(), target);
        for _ in 0..240 {
            s.run_ticks(4);
            let moved = sync.moved(s.sim());
            assert!(moved.len() % MOVED_STRIDE == 0);
            // Only a few props near the car may ever move.
            assert!(
                moved.len() / MOVED_STRIDE < 20,
                "{} moved",
                moved.len() / MOVED_STRIDE
            );
            for r in moved.chunks(MOVED_STRIDE) {
                if r[0] as usize == target {
                    seen = true;
                    drawn.copy_from_slice(&r[1..8]);
                }
            }
            // The renderer's copy (static list + moved records) matches the sim every frame.
            let truth = pose7(s.sim(), target);
            let err = ((drawn[0] - truth[0]).powi(2)
                + (drawn[1] - truth[1]).powi(2)
                + (drawn[2] - truth[2]).powi(2))
            .sqrt();
            assert!(err < 1e-3, "drawn pose off by {err} m");
        }
        assert!(seen, "the knocked parked car moved");
        // Replay-style reset of the sim: a fresh session puts every prop back, and a sync
        // that only knew the old session resends nothing it does not have to.
        let fresh = city_session();
        let back = sync.moved(fresh.sim());
        assert!(back.len() / MOVED_STRIDE <= 20);
    }

    #[test]
    fn a_knocked_parked_car_dents_and_reads_like_a_vehicle() {
        let mut s = city_session();
        let player = s.entities()[0];
        let target = s.debug_knock_prop(player, None, 14.0).unwrap();
        assert!(
            wreck_list(s.sim()).is_empty(),
            "nothing dented before the hit"
        );
        s.run_ticks(480);
        let list = wreck_list(s.sim());
        assert_eq!(list.len() % WRECK_STRIDE, 0);
        let rec = list
            .chunks(WRECK_STRIDE)
            .find(|r| r[0] as usize == target)
            .expect("the knocked parked car was promoted");
        assert!(rec[2] < 1.0, "health {}", rec[2]);
        for s in &rec[4..7] {
            assert!((0.9..1.1).contains(s), "scale {s}");
        }
        // Its entity id reads like a vehicle's: car model and a dented lattice.
        let id = wp_contracts::EntityId(wp_sim::PARKED_ENTITY_BASE + target as u32);
        let car = s
            .car(id)
            .expect("car model loaded for the dented parked car");
        let nodes = s.sim().node_positions(id).unwrap();
        assert_eq!(nodes.len(), car.lattice.nodes.len());
        let moved = nodes
            .iter()
            .zip(&car.lattice.nodes)
            .map(|(a, b)| {
                ((a[0] - b.pos_m[0]).powi(2)
                    + (a[1] - b.pos_m[1]).powi(2)
                    + (a[2] - b.pos_m[2]).powi(2))
                .sqrt()
            })
            .fold(0.0f32, f32::max);
        assert!(moved > 0.01, "lattice dented by {moved} m");
    }
}
