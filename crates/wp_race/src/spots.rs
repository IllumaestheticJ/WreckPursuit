//! Honk-to-start meet spots: one per picker race ([`crate::catalog`]) that can run on the
//! loaded city, at the race's start (the pole position of its planned course). A player in
//! free drive who stops inside a spot and sounds the horn starts that race with their own car.
//!
//! - [`race_spots`] plans every race that runs on the world's city ([`catalog::races_on`]) and
//!   places a spot at its pole position, facing the race direction. A race whose course does
//!   not plan (a POI or road missing) gets no spot.
//! - [`spot_at`] is the spot a car is standing in (horizontal radius, small height window so a
//!   car on an overpass does not trigger the spot below it).
//! - [`HonkTrigger`] turns the horn into a one-shot start request: rising edge only, inside a
//!   spot, nearly stopped. A held horn never retriggers.
//!
//! Pure, deterministic functions of the world and the car's state; nothing here touches the
//! simulation (starting the race is the caller's job, see `wp_wasm::session`).

use wp_world::World;

use crate::catalog::{self, RaceDef};

/// Radius of a meet spot, m (horizontal).
pub const SPOT_RADIUS_M: f32 = 12.0;

/// A car more than this far above or below a spot is not in it (overpasses, tunnels), m.
pub const SPOT_HEIGHT_WINDOW_M: f32 = 4.0;

/// The car must be slower than this for a honk to start the race, m/s.
pub const HONK_MAX_SPEED_MPS: f32 = 3.0;

/// One honk-to-start meet spot.
#[derive(Debug, Clone, PartialEq)]
pub struct RaceSpot {
    /// The race it starts (`catalog::race(race_id)`).
    pub race_id: &'static str,
    /// Spot centre: the race's pole position, m.
    pub pos: [f32; 3],
    /// Race direction at the start (yaw about +Z, 0 = +Y), rad.
    pub heading_rad: f32,
    /// Horizontal radius, m.
    pub radius_m: f32,
    /// Planned course length on this city (one lap for circuits), m.
    pub length_m: f32,
}

impl RaceSpot {
    /// The race this spot starts.
    pub fn race(&self) -> Option<RaceDef> {
        catalog::race(self.race_id)
    }

    /// A car at `pos` stands in this spot.
    pub fn contains(&self, pos: [f32; 3]) -> bool {
        let (dx, dy) = (pos[0] - self.pos[0], pos[1] - self.pos[1]);
        dx * dx + dy * dy <= self.radius_m * self.radius_m
            && (pos[2] - self.pos[2]).abs() <= SPOT_HEIGHT_WINDOW_M
    }
}

/// The spot of one race on `world`, if its course plans there.
pub fn race_spot(world: &World, def: &RaceDef) -> Option<RaceSpot> {
    let race = catalog::plan_course(world, def)?;
    let (pos, dir) = race.course.start();
    Some(RaceSpot {
        race_id: def.id,
        pos,
        heading_rad: wp_world::geom::yaw_of(dir),
        radius_m: SPOT_RADIUS_M,
        length_m: race.route.length_m,
    })
}

/// Every race spot on `world` (races that run on its city, in menu order; races whose course
/// does not plan on this world are left out).
pub fn race_spots(world: &World) -> Vec<RaceSpot> {
    catalog::races_on(&world.network().id)
        .iter()
        .filter_map(|d| race_spot(world, d))
        .collect()
}

/// The spot a car at `pos` stands in (the nearest centre when spots overlap).
pub fn spot_at(spots: &[RaceSpot], pos: [f32; 3]) -> Option<&RaceSpot> {
    spots.iter().filter(|s| s.contains(pos)).min_by(|a, b| {
        let d = |s: &RaceSpot| (pos[0] - s.pos[0]).hypot(pos[1] - s.pos[1]);
        d(a).total_cmp(&d(b))
    })
}

/// Horn → race start request. Feed it the player's horn every tick; it fires once per press.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HonkTrigger {
    horn_was_down: bool,
}

impl HonkTrigger {
    /// The horn's state last tick.
    pub fn horn_was_down(&self) -> bool {
        self.horn_was_down
    }

    /// The horn was pressed this tick (rising edge). Updates the edge state.
    pub fn pressed(&mut self, horn: bool) -> bool {
        let edge = horn && !self.horn_was_down;
        self.horn_was_down = horn;
        edge
    }

    /// One tick: the race to start, if the horn was just pressed (rising edge) by a car at
    /// `pos` moving at `speed_mps` inside one of `spots` and slower than
    /// [`HONK_MAX_SPEED_MPS`]. `spots` is only called on a press (planning them is not free).
    pub fn update<'a>(
        &mut self,
        horn: bool,
        pos: [f32; 3],
        speed_mps: f32,
        spots: impl FnOnce() -> &'a [RaceSpot],
    ) -> Option<&'static str> {
        if !self.pressed(horn) || speed_mps >= HONK_MAX_SPEED_MPS {
            return None;
        }
        spot_at(spots(), pos).map(|s| s.race_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spot(id: &'static str, pos: [f32; 3]) -> RaceSpot {
        RaceSpot {
            race_id: id,
            pos,
            heading_rad: 0.0,
            radius_m: SPOT_RADIUS_M,
            length_m: 1000.0,
        }
    }

    #[test]
    fn spot_contains_inside_the_radius_and_height_window() {
        let s = spot("a", [10.0, 20.0, 5.0]);
        assert!(s.contains([10.0, 20.0, 5.0]));
        assert!(s.contains([21.9, 20.0, 6.0]));
        assert!(!s.contains([22.1, 20.0, 5.0]));
        // An overpass above the spot is not in it.
        assert!(!s.contains([10.0, 20.0, 12.0]));
    }

    #[test]
    fn overlapping_spots_pick_the_nearest() {
        let spots = [spot("a", [0.0; 3]), spot("b", [10.0, 0.0, 0.0])];
        assert_eq!(spot_at(&spots, [2.0, 0.0, 0.0]).unwrap().race_id, "a");
        assert_eq!(spot_at(&spots, [8.0, 0.0, 0.0]).unwrap().race_id, "b");
        assert!(spot_at(&spots, [40.0, 0.0, 0.0]).is_none());
    }

    #[test]
    fn honk_fires_once_per_press_inside_a_spot_when_stopped() {
        let spots = vec![spot("a", [0.0; 3])];
        let mut t = HonkTrigger::default();
        let mut tick = |t: &mut HonkTrigger, horn, pos, v| t.update(horn, pos, v, || &spots);
        // Outside the spot: nothing.
        assert_eq!(tick(&mut t, true, [50.0, 0.0, 0.0], 0.0), None);
        assert_eq!(tick(&mut t, false, [50.0, 0.0, 0.0], 0.0), None);
        // Inside but moving: nothing, and holding the horn while slowing down does not fire.
        assert_eq!(tick(&mut t, true, [1.0, 0.0, 0.0], 8.0), None);
        assert_eq!(tick(&mut t, true, [1.0, 0.0, 0.0], 0.5), None);
        assert_eq!(tick(&mut t, false, [1.0, 0.0, 0.0], 0.5), None);
        // A fresh press while stopped inside: fires once, held horn does not retrigger.
        assert_eq!(tick(&mut t, true, [1.0, 0.0, 0.0], 0.5), Some("a"));
        for _ in 0..100 {
            assert_eq!(tick(&mut t, true, [1.0, 0.0, 0.0], 0.0), None);
        }
        assert_eq!(tick(&mut t, false, [1.0, 0.0, 0.0], 0.0), None);
        assert_eq!(tick(&mut t, true, [1.0, 0.0, 0.0], 0.0), Some("a"));
    }

    #[test]
    fn spots_are_only_planned_on_a_press() {
        let mut t = HonkTrigger::default();
        let mut calls = 0;
        let empty: Vec<RaceSpot> = Vec::new();
        for horn in [false, true, true, false, false, true] {
            t.update(horn, [0.0; 3], 0.0, || {
                calls += 1;
                &empty
            });
        }
        assert_eq!(calls, 2);
    }

    #[test]
    fn grid_city_has_its_race_spot_at_the_pole() {
        let world = World::procedural(
            &wp_world::procgen::ProcGenConfig::default(),
            wp_world::WorldConfig::default(),
        );
        let spots = race_spots(&world);
        assert_eq!(
            spots.iter().map(|s| s.race_id).collect::<Vec<_>>(),
            vec!["grid_highway_sprint"]
        );
        let def = catalog::race("grid_highway_sprint").unwrap();
        let race = catalog::plan_course(&world, &def).unwrap();
        assert_eq!(spots[0].pos, catalog::venue_pos(&race));
        assert!((spots[0].length_m - def.length_hint_m).abs() < 0.02 * def.length_hint_m);
    }
}
