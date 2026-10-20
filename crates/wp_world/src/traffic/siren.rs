//! Yielding to emergency vehicles (owner playtest rows 61–62: "traffic will get out of the way
//! of cops if siren is on").
//!
//! Any tracked vehicle with its siren on (AI police units, a player cop in multiplayer) is a
//! *siren unit*. Each civilian classifies every siren unit near it ([`classify`]):
//!
//! - **Behind** — coming up behind in the same direction, moving, within
//!   [`behind_range_m`] (60–120 m by the unit's speed). The driver notices it after a reaction
//!   time (0.5–2 s by personality, [`reaction_s`]), signals, slows down and pulls to the kerb
//!   (or, on a multi-lane road, first changes out of the lane the unit is in), stops, waits until
//!   the unit has gone past and [`CLEAR_S`] more, then signals and merges back once nobody is
//!   coming up behind it in the lane ([`YieldPhase`]).
//! - **Oncoming** — coming the other way within [`ONCOMING_RANGE_M`]: the driver moves to the
//!   kerb side of its lane and slows to [`ONCOMING_MPS`] until the unit has gone past.
//! - **At a junction** — a unit within [`JUNCTION_RANGE_M`] of the junction a driver is about
//!   to enter, heading for it: the driver does not enter (stops at the line if it still can;
//!   cars already inside keep going so the box clears). Never blocks a junction to make room.
//!
//! A pull-over keeps the car inside its own lane: [`kerb_offset_m`] puts the body 0.2 m from the
//! lane edge (0.65 m right of the lane centre on a 3.5 m lane), so a unit straddling the centre
//! line has a 2.6 m corridor between the pulled-over cars of both directions (a 1.9 m car
//! passes with 0.35 m each side).
//!
//! Kinematic (far LOD) and physical agents run the same decision code, so far cars are already
//! on the kerb when they are promoted. Cost: one distance test per siren unit per agent per
//! traffic step. Deterministic: no RNG draws (the reaction time is a hash of id + seed).

use wp_math::StateHasher;

use super::{AgentId, Personality, TrackedVehicle, HALF_L};
use crate::geom::{self, V2};

/// A unit behind is noticed at most this far back, m (at ≥ 20 m/s; see [`behind_range_m`]).
pub const BEHIND_RANGE_MAX_M: f32 = 120.0;
/// A unit coming the other way is reacted to within this distance ahead, m.
pub const ONCOMING_RANGE_M: f32 = 100.0;
/// Oncoming cars slow to this speed while a unit comes past, m/s.
pub const ONCOMING_MPS: f32 = 4.0;
/// Drivers do not enter a junction while a moving siren unit heading for it is this close to it
/// (or 6 s of its travel, up to [`JUNCTION_RANGE_MAX_M`]), m.
pub const JUNCTION_RANGE_M: f32 = 70.0;
/// Only drivers this close to their stop line consider the junction hold, m.
pub const JUNCTION_APPROACH_M: f32 = 60.0;
/// Upper limit of the junction hold range for fast units, m.
pub const JUNCTION_RANGE_MAX_M: f32 = 120.0;
/// A unit must move at least this fast to be "coming" (a parked unit at a scene is not), m/s.
pub const MOVING_MPS: f32 = 2.0;
/// After the unit has gone past, a yielding driver waits this long before merging back, s.
pub const CLEAR_S: f32 = 2.0;
/// A unit a driver has pulled over for keeps it there while it is this close behind, even when
/// it slows down or stops (it is waiting for room to get past), m.
pub const HOLD_RANGE_M: f32 = 40.0;
/// A pulled-over driver gives up on a unit that has stood still behind it this long, s (and
/// does not pull over for it again until it moves).
pub const STALL_S: f32 = 15.0;
/// A slow or stopped unit this close behind still makes a driver pull over (a unit stuck behind
/// it, waiting for room), m.
pub const START_CLOSE_M: f32 = 30.0;
/// Drivers slow to [`NEAR_MPS`] passing a stopped unit with its lights on this close, m.
pub const NEAR_M: f32 = 30.0;
/// Speed past a stopped unit with its lights on, m/s.
pub const NEAR_MPS: f32 = 8.0;
/// Room kept between the car body and the lane edge when pulled over, m.
pub const KERB_MARGIN_M: f32 = 0.15;
/// Crawl speed while still moving sideways onto the kerb, m/s.
pub const PULL_OVER_CRAWL_MPS: f32 = 2.0;

/// Where a driver is in its yield to a siren unit coming up behind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum YieldPhase {
    /// Not yielding.
    #[default]
    None,
    /// The unit is there but the driver has not reacted yet (reaction time).
    Noticing,
    /// Signalling, slowing and moving to the kerb.
    PullingOver,
    /// Stopped at the kerb until the unit has passed (+ [`CLEAR_S`]).
    Waiting,
    /// Signalling back into the lane once it is clear behind.
    MergingBack,
}

/// Per-driver yield state.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SirenYield {
    /// Phase.
    pub phase: YieldPhase,
    /// Time in the current phase (Noticing / Waiting: time since the unit cleared), s.
    pub timer_s: f32,
    /// Side the car pulls to: +1 right (kerb), −1 left (away from a unit on the right).
    pub side: f32,
    /// How long the unit has stood still behind the waiting car, s.
    pub stall_s: f32,
}

impl SirenYield {
    /// Whether the driver is reacting to a unit behind it (not merely noticing it).
    pub fn active(&self) -> bool {
        matches!(
            self.phase,
            YieldPhase::PullingOver | YieldPhase::Waiting | YieldPhase::MergingBack
        )
    }

    /// Add to a state hash (only called for agents not in [`YieldPhase::None`]).
    pub fn hash_into(&self, h: &mut StateHasher) {
        h.write_u32(self.phase as u32);
        h.write_f32_q(self.timer_s, 1e3);
        h.write_f32_q(self.side, 1e3);
        h.write_f32_q(self.stall_s, 1e3);
    }
}

/// How one siren unit relates to a civilian.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SirenRel {
    /// Coming up behind in the same direction.
    Behind {
        /// Distance behind (centre to centre, along the car's heading), m.
        back_m: f32,
        /// The unit's lateral offset from the car (+ right), m.
        lat_m: f32,
        /// The unit's speed, m/s.
        speed_mps: f32,
    },
    /// Coming the other way ahead.
    Oncoming {
        /// Distance ahead, m.
        ahead_m: f32,
    },
    /// Stopped or crawling nearby (a scene, a road block), not behind.
    Near,
}

/// Heading of a tracked vehicle: its velocity direction when moving, else its forward.
pub fn heading_of(t: &TrackedVehicle) -> V2 {
    let v = [t.vel[0], t.vel[1]];
    if geom::dot(v, v) > 1.0 {
        geom::normalize(v)
    } else if geom::dot(t.forward, t.forward) > 0.25 {
        geom::normalize(t.forward)
    } else {
        [0.0, 0.0]
    }
}

/// How far back a unit moving at `speed` m/s is noticed: 60 m when slow, 120 m from 15 m/s.
pub fn behind_range_m(speed: f32) -> f32 {
    (30.0 + 6.0 * speed).clamp(60.0, BEHIND_RANGE_MAX_M)
}

/// Classify siren unit `t` relative to a civilian at `q` (height `z`) facing `f`. `None` for
/// moving units that are not coming towards it along its road (ahead moving away, crossing)
/// and for anything on another level. A slow or stopped unit behind within [`HOLD_RANGE_M`] is
/// still `Behind` (with its speed): a driver already pulled over keeps waiting for it, but only
/// a moving one or one within [`START_CLOSE_M`] makes a driver start ([`starts_yield`]). Any
/// other slow unit within [`NEAR_M`] is `Near`.
pub fn classify(t: &TrackedVehicle, q: V2, z: f32, f: V2) -> Option<SirenRel> {
    if !t.siren || (t.pos[2] - z).abs() > 4.0 {
        return None;
    }
    let r = geom::sub(geom::xy(t.pos), q);
    if geom::dot(r, r) > BEHIND_RANGE_MAX_M * BEHIND_RANGE_MAX_M {
        return None;
    }
    let speed = geom::len([t.vel[0], t.vel[1]]);
    let long = geom::dot(r, f);
    let lat = geom::dot(r, geom::right_of(f));
    let along = geom::dot(heading_of(t), f);
    let range = if speed < MOVING_MPS {
        HOLD_RANGE_M
    } else {
        behind_range_m(speed)
    };
    if along > 0.5 && long < HALF_L && -long < range && lat.abs() < 7.5 {
        return Some(SirenRel::Behind {
            back_m: -long,
            lat_m: lat,
            speed_mps: speed,
        });
    }
    // Coming the other way (or facing this car close ahead, e.g. turning into its street).
    if (speed >= MOVING_MPS || long < START_CLOSE_M)
        && along < -0.5
        && long > -HALF_L
        && long < ONCOMING_RANGE_M
        && lat.abs() < 12.0
    {
        return Some(SirenRel::Oncoming { ahead_m: long });
    }
    if speed < MOVING_MPS && geom::dot(r, r) < NEAR_M * NEAR_M {
        return Some(SirenRel::Near);
    }
    None
}

/// Whether a unit behind (`rel`) makes a driver not yet yielding start to: it must be moving or
/// close ([`START_CLOSE_M`]).
pub fn starts_yield(rel: &SirenRel) -> bool {
    matches!(rel, SirenRel::Behind { speed_mps, back_m, .. }
        if *speed_mps >= MOVING_MPS || *back_m < START_CLOSE_M)
}

/// Side a driver pulls to for a unit `back_m` behind it and `lat_m` to its right (negative =
/// left): the kerb (+1), unless the unit is close ([`SIDE_RANGE_M`]: further back, on a bend,
/// the straight-line offset says nothing) and coming up on its right (more than 0.5 m right of
/// it: e.g. riding the line to the lane on the right) — then away from it, to the left.
pub fn side_for(back_m: f32, lat_m: f32) -> f32 {
    if back_m < SIDE_RANGE_M && lat_m > 0.5 {
        -1.0
    } else {
        1.0
    }
}

/// A unit's lateral offset decides the pull-over side only this close behind, m.
pub const SIDE_RANGE_M: f32 = 35.0;

/// Whether siren unit `t` is heading for the junction whose stop line is at `line` (height `z`)
/// closely enough that a driver must not enter it.
pub fn heads_for_junction(t: &TrackedVehicle, line: V2, z: f32) -> bool {
    if !t.siren || (t.pos[2] - z).abs() > 4.0 {
        return false;
    }
    let r = geom::sub(line, geom::xy(t.pos));
    let d2 = geom::dot(r, r);
    let speed = geom::len([t.vel[0], t.vel[1]]);
    // A unit parked at a scene does not close the junction (drivers slow past it instead):
    // only a moving one does.
    if speed < MOVING_MPS {
        return false;
    }
    let range = (speed * 6.0).clamp(JUNCTION_RANGE_M, JUNCTION_RANGE_MAX_M);
    if d2 > range * range {
        return false;
    }
    d2 < 20.0 * 20.0 || geom::dot(heading_of(t), r) > 0.0
}

/// Lateral offset from the lane centre that puts a car's body [`KERB_MARGIN_M`] from the edge
/// of a lane `lane_width_m` wide, m (≥ 0.3, ≤ 1.2).
pub fn kerb_offset_m(lane_width_m: f32) -> f32 {
    (lane_width_m * 0.5 - super::CAR_WIDTH_M * 0.5 - KERB_MARGIN_M).clamp(0.3, 1.2)
}

/// Reaction time before a driver starts pulling over, s: law-abiding 0.5–1.2, casual 0.7–1.6,
/// aggressive 1.0–2.0 (derived from the id and the traffic seed: no RNG draws).
pub fn reaction_s(seed: u64, id: AgentId, p: Personality) -> f32 {
    let mut x = seed ^ (id as u64).wrapping_mul(0xD1B5_4A32_D192_ED03) ^ 0x51DE_0BAD;
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^= x >> 31;
    let u = (x >> 40) as f32 / (1u64 << 24) as f32;
    match p {
        Personality::LawAbiding => 0.5 + 0.7 * u,
        Personality::Casual => 0.7 + 0.9 * u,
        Personality::Aggressive => 1.0 + 1.0 * u,
    }
}

/// Acceleration that brings a car at `v` down to `target` (≥ 1 m/s² until stopped, at most
/// `max_decel`), or up to it gently, m/s².
pub fn approach_speed(v: f32, target: f32, max_decel: f32, max_accel: f32) -> f32 {
    if v > target + 0.05 {
        -((v - target) * 0.8).max(1.0).min(max_decel)
    } else if target <= 0.0 {
        -1.0
    } else {
        ((target - v) * 0.8).min(max_accel)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wp_contracts::EntityId;

    fn unit(pos: [f32; 2], vel: [f32; 2], siren: bool) -> TrackedVehicle {
        TrackedVehicle {
            entity: EntityId(9),
            pos: [pos[0], pos[1], 0.0],
            vel: [vel[0], vel[1], 0.0],
            forward: if vel == [0.0, 0.0] {
                [0.0, 1.0]
            } else {
                geom::normalize(vel)
            },
            siren,
            player: false,
        }
    }

    const Q: V2 = [0.0, 0.0];
    const F: V2 = [0.0, 1.0];

    #[test]
    fn unit_coming_up_behind_is_behind() {
        let t = unit([0.0, -80.0], [0.0, 16.0], true);
        let rel = classify(&t, Q, 0.0, F);
        assert_eq!(
            rel,
            Some(SirenRel::Behind {
                back_m: 80.0,
                lat_m: 0.0,
                speed_mps: 16.0
            })
        );
        assert!(starts_yield(&rel.unwrap()));
        // Lights off: nothing.
        let t = unit([0.0, -80.0], [0.0, 16.0], false);
        assert_eq!(classify(&t, Q, 0.0, F), None);
    }

    #[test]
    fn range_behind_grows_with_unit_speed() {
        assert_eq!(behind_range_m(0.0), 60.0);
        assert_eq!(behind_range_m(30.0), 120.0);
        let slow = unit([0.0, -90.0], [0.0, 3.0], true);
        assert_eq!(classify(&slow, Q, 0.0, F), None);
        let fast = unit([0.0, -90.0], [0.0, 20.0], true);
        assert!(matches!(
            classify(&fast, Q, 0.0, F),
            Some(SirenRel::Behind { .. })
        ));
    }

    #[test]
    fn parked_ahead_crossing_and_other_levels_are_ignored() {
        // Stopped behind: far back it is nothing; 30-40 m back it holds a driver that is
        // already pulled over but does not make anyone start; closer it does (it is stuck
        // behind the car).
        assert_eq!(
            classify(&unit([0.0, -50.0], [0.0, 0.0], true), Q, 0.0, F),
            None
        );
        let held = classify(&unit([0.0, -35.0], [0.0, 0.0], true), Q, 0.0, F).unwrap();
        assert!(matches!(held, SirenRel::Behind { .. }));
        assert!(!starts_yield(&held));
        let stuck = classify(&unit([0.0, -10.0], [0.0, 1.0], true), Q, 0.0, F).unwrap();
        assert!(starts_yield(&stuck));
        // Stopped ahead / beside: slow down past it.
        assert_eq!(
            classify(&unit([-3.5, 20.0], [0.0, 0.0], true), Q, 0.0, F),
            Some(SirenRel::Near)
        );
        // Ahead, moving away (and not near).
        assert_eq!(
            classify(&unit([0.0, 30.0], [0.0, 15.0], true), Q, 0.0, F),
            None
        );
        // Crossing street.
        assert_eq!(
            classify(&unit([-30.0, 20.0], [15.0, 0.0], true), Q, 0.0, F),
            None
        );
        // On a bridge above.
        let mut t = unit([0.0, -30.0], [0.0, 15.0], true);
        t.pos[2] = 8.0;
        assert_eq!(classify(&t, Q, 0.0, F), None);
    }

    #[test]
    fn oncoming_unit_ahead() {
        let t = unit([-3.5, 60.0], [0.0, -15.0], true);
        assert_eq!(
            classify(&t, Q, 0.0, F),
            Some(SirenRel::Oncoming { ahead_m: 60.0 })
        );
        // Stopped facing this car close ahead: make room as for an oncoming one.
        let mut t = unit([-1.0, 12.0], [0.0, 0.0], true);
        t.forward = [0.0, -1.0];
        assert_eq!(
            classify(&t, Q, 0.0, F),
            Some(SirenRel::Oncoming { ahead_m: 12.0 })
        );
        // Already past.
        let t = unit([-3.5, -10.0], [0.0, -15.0], true);
        assert_eq!(classify(&t, Q, 0.0, F), None);
    }

    #[test]
    fn junction_hold_for_units_heading_there() {
        let line = [0.0, 50.0];
        // Cross street, 40 m from the junction, driving towards it.
        assert!(heads_for_junction(
            &unit([-40.0, 50.0], [15.0, 0.0], true),
            line,
            0.0
        ));
        // Driving away from it.
        assert!(!heads_for_junction(
            &unit([-40.0, 50.0], [-15.0, 0.0], true),
            line,
            0.0
        ));
        // Right at the junction and moving (turning): hold; parked there: no.
        assert!(heads_for_junction(
            &unit([-5.0, 50.0], [3.0, -3.0], true),
            line,
            0.0
        ));
        assert!(!heads_for_junction(
            &unit([-5.0, 50.0], [0.0, 0.0], true),
            line,
            0.0
        ));
        // Far away or lights off.
        assert!(!heads_for_junction(
            &unit([-90.0, 50.0], [10.0, 0.0], true),
            line,
            0.0
        ));
        // A fast unit is held for from further out.
        assert!(heads_for_junction(
            &unit([-90.0, 50.0], [18.0, 0.0], true),
            line,
            0.0
        ));
        assert!(!heads_for_junction(
            &unit([-40.0, 50.0], [15.0, 0.0], false),
            line,
            0.0
        ));
    }

    #[test]
    fn pull_away_from_the_unit() {
        assert_eq!(side_for(20.0, 0.0), 1.0);
        assert_eq!(side_for(20.0, -1.7), 1.0);
        assert_eq!(side_for(20.0, 1.3), -1.0);
        // Far back (maybe on a bend): the kerb.
        assert_eq!(side_for(80.0, 1.95), 1.0);
    }

    #[test]
    fn kerb_offset_keeps_the_body_in_the_lane() {
        let k = kerb_offset_m(3.5);
        assert!((k - 0.65).abs() < 1e-5, "{k}");
        assert!(k + super::super::CAR_WIDTH_M * 0.5 < 1.75);
        assert_eq!(kerb_offset_m(2.0), 0.3);
        assert_eq!(kerb_offset_m(6.0), 1.2);
    }

    #[test]
    fn reaction_times_by_personality() {
        for id in 0..500 {
            let l = reaction_s(7, id, Personality::LawAbiding);
            let c = reaction_s(7, id, Personality::Casual);
            let a = reaction_s(7, id, Personality::Aggressive);
            assert!((0.5..=1.2).contains(&l), "{l}");
            assert!((0.7..=1.6).contains(&c), "{c}");
            assert!((1.0..=2.0).contains(&a), "{a}");
        }
        assert_eq!(
            reaction_s(3, 11, Personality::Casual),
            reaction_s(3, 11, Personality::Casual)
        );
        assert_ne!(
            reaction_s(3, 11, Personality::Casual),
            reaction_s(4, 11, Personality::Casual)
        );
    }

    #[test]
    fn approach_speed_brakes_then_holds() {
        assert!(approach_speed(12.0, 0.0, 3.0, 2.0) == -3.0);
        assert!(approach_speed(1.0, 0.0, 3.0, 2.0) == -1.0);
        assert_eq!(approach_speed(0.0, 0.0, 3.0, 2.0), -1.0);
        assert!(approach_speed(0.0, 2.0, 3.0, 2.0) > 0.0);
    }
}
