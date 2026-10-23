//! Honk-to-start race spots in the browser session (`wp_race::spots`).
//!
//! In a live free-drive or city-chase session on a city (not a race, replay, scripted menu run
//! or career run), every picker race that can run on the session's city has a meet spot at its
//! start. When the player's car stands in a spot (slower than
//! `wp_race::spots::HONK_MAX_SPEED_MPS`) and the horn is pressed (rising edge of
//! `VehicleInput::horn`, checked every tick after the step), the session records the race as
//! *honked*. The testbed polls `race_spot_honked()` and starts it with the player's car and
//! damage mode exactly like the picker's "Race" button (`race_start`, which keeps the player
//! on the current map when the race runs there, e.g. a Port Vesper race on the `ashpeak` map).
//!
//! The check reads the session's state after each tick and never writes to the sim, so state
//! hashes, replays and recordings are unchanged. Spots are planned lazily (the first
//! `race_spots()` call or the first horn press).

use serde::Serialize;
use wp_race::catalog;
use wp_race::spots::{race_spots, HonkTrigger, RaceSpot};
use wp_world::World;

/// The session's meet spots and horn trigger.
#[derive(Debug, Clone, Default)]
pub struct MeetSpots {
    spots: Option<Vec<RaceSpot>>,
    trigger: HonkTrigger,
    honked: Option<&'static str>,
}

impl MeetSpots {
    /// The spots on `world` (planned on first use).
    pub fn spots(&mut self, world: &World) -> &[RaceSpot] {
        self.spots.get_or_insert_with(|| race_spots(world))
    }

    /// One tick of the player's car: horn, position, speed. Records the race when the horn was
    /// just pressed in a spot at a near stop (the first such race stays until taken).
    pub fn tick(&mut self, world: &World, horn: bool, pos: [f32; 3], speed_mps: f32) {
        let spots = &mut self.spots;
        let hit = self.trigger.update(horn, pos, speed_mps, move || {
            spots.get_or_insert_with(|| race_spots(world)).as_slice()
        });
        if self.honked.is_none() {
            self.honked = hit;
        }
    }

    /// Track the horn without triggering (the session is not eligible this tick), so a horn
    /// held into an eligible tick is not a fresh press.
    pub fn track_horn(&mut self, horn: bool) {
        self.trigger.pressed(horn);
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
