//! Honk-to-start race spots in the browser session (`wp_race::spots`).
//!
//! In a live free-drive or night-run session on a city (not a race, replay, scripted menu run
//! or career run), every picker race that can run on the session's city has a meet spot at its
//! start. Spots are part of round 12's one challenge system on the horn action
//! (`crate::street`): the session reads one horn edge per tick and a street racer within
//! honking range takes it first; otherwise, when the player's car stands in a spot (slower
//! than `wp_race::spots::HONK_MAX_SPEED_MPS`), the session records the race as *honked*. The
//! same rules apply as to a street challenge: not while the player is wanted or chased, not
//! with a street challenge or street race open, not with the run over.
//!
//! The testbed polls `race_spot_honked()` and starts it with the player's car and damage mode
//! exactly like the picker's "Race" button (`race_start`, which keeps the player on the current
//! map when the race runs there, e.g. a Port Vesper race on the `ashpeak` map). The money is
//! the picker race's existing stake (advertised buy-in and purse, `race_list`): no second
//! betting system.
//!
//! The check reads the session's state and never writes to the sim, so state hashes, replays
//! and recordings are unchanged. Spots are planned lazily (the first `race_spots()` call or
//! the first honk).

use serde::Serialize;
use wp_race::catalog;
use wp_race::spots::{honk_spot, race_spots, RaceSpot};
use wp_world::World;

/// The session's meet spots and the race honked for.
#[derive(Debug, Clone, Default)]
pub struct MeetSpots {
    spots: Option<Vec<RaceSpot>>,
    honked: Option<&'static str>,
}

impl MeetSpots {
    /// The spots on `world` (planned on first use).
    pub fn spots(&mut self, world: &World) -> &[RaceSpot] {
        self.spots.get_or_insert_with(|| race_spots(world))
    }

    /// A honk by the player's car at `pos` moving at `speed_mps`: records the race of the spot
    /// it stands in when nearly stopped (the first such race stays until taken). Returns
    /// whether it hit a spot.
    pub fn honk(&mut self, world: &World, pos: [f32; 3], speed_mps: f32) -> bool {
        let hit = honk_spot(self.spots(world), pos, speed_mps).map(|s| s.race_id);
        if self.honked.is_none() {
            self.honked = hit;
        }
        hit.is_some()
    }

    /// Take the honked race id, if any.
    pub fn take_honked(&mut self) -> Option<&'static str> {
        self.honked.take()
    }

    /// The honked race id waiting to be taken, if any.
    pub fn honked(&self) -> Option<&'static str> {
        self.honked
    }
}

/// One spot for the testbed (`race_spots()`).
#[derive(Debug, Clone, Serialize)]
pub struct SpotCard {
    /// Race id (`race_start`).
    pub id: &'static str,
    /// Race name.
    pub name: &'static str,
    /// Kind key (`sprint`, `circuit`, `checkpoint`, `drag`, …).
    pub kind: &'static str,
    /// Laps (1 for point to point).
    pub laps: u32,
    /// Legality key (`street`, `illegal_daytime`).
    pub legality: &'static str,
    /// Spot centre (the race's pole position), m.
    pub x: f32,
    /// Spot centre y, m.
    pub y: f32,
    /// Spot centre z, m.
    pub z: f32,
    /// Race direction (yaw about +Z, 0 = +Y), rad.
    pub heading: f32,
    /// Radius, m.
    pub radius: f32,
    /// Course length on this map (one lap), m.
    pub length_m: f32,
}

/// The testbed's view of `spots`.
pub fn spot_cards(spots: &[RaceSpot]) -> Vec<SpotCard> {
    spots
        .iter()
        .filter_map(|s| {
            let d = catalog::race(s.race_id)?;
            Some(SpotCard {
                id: d.id,
                name: d.name,
                kind: d.kind.key(),
                laps: d.laps(),
                legality: d.legality.key(),
                x: s.pos[0],
                y: s.pos[1],
                z: s.pos[2],
                heading: s.heading_rad,
                radius: s.radius_m,
                length_m: s.length_m,
            })
        })
        .collect()
}
