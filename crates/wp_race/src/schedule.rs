//! Event scheduler: the day's calendar from seeded rules (`data/events/schedule.toml`).
//!
//! - **Sanctioned** daytime events on closed routes: traffic suppressed, cones, marshals.
//! - **Night meets** at venues (race-start POIs): spawn time, lobby window, start.
//! - **Illegal daytime** street races: rare, very high purse, police presence forced to max.
//!
//! The calendar for day `d` depends only on `(seed, d, venues, meet heat)`, so every server and
//! client generates the same one. Times are integer seconds since midnight of the calendar's
//! day; night meets after midnight have `start_s >= 86 400` (they belong to that night).
//!
//! Meets have a rolling heat cost per venue ([`MeetHeat`]): each race adds heat, heat decays per
//! hour, and a hot venue attracts a patrol ([`RaceSignal::PatrolAttracted`]) and is picked less
//! often by the scheduler.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use wp_contracts::{DayPhase, TimeOfDay};
use wp_math::Rng;

use crate::error::RaceDataError;
use crate::kind::{ClassBracket, DistrictId, EventId, Legality, RaceKind, VenueId};
use crate::progression::EntryRequirements;
use crate::signals::{PolicePresence, RaceSignal};
use crate::stakes::{Stake, StakeTable};

/// Seconds per in-game day (integer form of `wp_contracts::SECONDS_PER_DAY`).
pub const DAY_S: u32 = 86_400;

/// A place races can start from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Venue {
    /// Id.
    pub id: VenueId,
    /// Display name.
    pub name: String,
    /// Pole position, m.
    pub pos: [f32; 3],
    /// District the venue sits in.
    pub district: Option<DistrictId>,
    /// A closed circuit can be run from here (sanctioned day events).
    pub closed_circuit: bool,
}

/// Settings for one class of scheduled events.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SlotRules {
    /// Events per day, inclusive range `[min, max]`.
    pub per_day: [u32; 2],
    /// Start window in hours since midnight `[from, to)`; `to` may exceed 24 (after midnight).
    pub hours: [f32; 2],
    /// Kind weights, keyed by [`RaceKind::key`].
    pub kinds: BTreeMap<String, u32>,
    /// Cars gather at the venue this long before the start, minutes.
    pub spawn_lead_min: u32,
    /// Lobby opens this long before the start, minutes.
    pub lobby_open_min: u32,
    /// Minimum licence tier.
    pub min_licence: u8,
    /// Minimum personal rep.
    pub min_rep: i64,
}

/// Illegal daytime races.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IllegalDaytimeRules {
    /// Chance of one such race on a given day.
    pub chance_per_day: f32,
    /// The rest of the settings (`per_day` is ignored: at most one).
    #[serde(flatten)]
    pub slot: SlotRules,
}

/// Meet heat settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MeetHeatRules {
    /// Heat added per race at a venue.
    pub per_race: f32,
    /// Heat lost per in-game hour.
    pub decay_per_hour: f32,
    /// At or above this the venue attracts a patrol.
    pub patrol_threshold: f32,
    /// Scheduling weight multiplier for a venue at or above the threshold.
    pub hot_weight: f32,
}

/// The whole `schedule.toml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScheduleRules {
    /// Sanctioned day events.
    pub sanctioned: SlotRules,
    /// Night meets.
    pub night_meets: SlotRules,
    /// Rare illegal daytime races.
    pub illegal_daytime: IllegalDaytimeRules,
    /// Bracket weights, keyed by letter.
    pub bracket_weights: BTreeMap<String, u32>,
    /// Circuit laps range for scheduled circuits `[min, max]`.
    pub circuit_laps: [u8; 2],
    /// Marshals placed per sanctioned event.
    pub marshals: u8,
    /// Minimum minutes between two events at the same venue.
    pub venue_gap_min: u32,
    /// Meet heat.
    pub meet_heat: MeetHeatRules,
}

/// Traffic treatment for an event (`wp_world::ActiveRoute::closed` for `Suppressed`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrafficMode {
    /// Route closed: civilian traffic suppressed, cones on side streets.
    Suppressed,
    /// Normal city traffic.
    Normal,
    /// Extra traffic density (cannonball).
    Heavy,
}

/// One event on the calendar.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScheduledEvent {
    /// Id (`day * 1000 + n`).
    pub id: EventId,
    /// Calendar day.
    pub day: u32,
    /// Race kind.
    pub kind: RaceKind,
    /// Legality.
    pub legality: Legality,
    /// Venue.
    pub venue: VenueId,
    /// Car bracket.
    pub bracket: ClassBracket,
    /// Day phase at the start.
    pub phase: DayPhase,
    /// Cars gather (AI entrants spawn), seconds since midnight of `day`.
    pub spawn_s: u32,
    /// Lobby opens.
    pub lobby_open_s: u32,
    /// Countdown starts.
    pub start_s: u32,
    /// Stakes.
    pub stake: Stake,
    /// Police presence override for `wp_police` (`Max` for illegal daytime races).
    pub police_presence_override: PolicePresence,
    /// Traffic treatment.
    pub traffic: TrafficMode,
    /// Static marshal entities to place along the route (sanctioned only).
    pub marshals: u8,
    /// Entry requirements.
    pub requirements: EntryRequirements,
}

impl ScheduledEvent {
    /// Absolute in-game second of the start (`day * 86 400 + start_s`).
    pub fn start_abs_s(&self) -> u64 {
        self.day as u64 * DAY_S as u64 + self.start_s as u64
    }

    /// Absolute in-game second the lobby opens.
    pub fn lobby_abs_s(&self) -> u64 {
        self.day as u64 * DAY_S as u64 + self.lobby_open_s as u64
    }
}

/// A day's events, ascending by start then id.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DayCalendar {
    /// Day.
    pub day: u32,
    /// Events.
    pub events: Vec<ScheduledEvent>,
}

impl DayCalendar {
    /// Events whose lobby opens in `[from_abs_s, to_abs_s)`.
    pub fn lobbies_opening(&self, from_abs_s: u64, to_abs_s: u64) -> Vec<&ScheduledEvent> {
        self.events
            .iter()
            .filter(|e| (from_abs_s..to_abs_s).contains(&e.lobby_abs_s()))
            .collect()
    }
}

/// Rolling meet heat per venue.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct MeetHeat {
    /// `(heat, absolute second of the last update)` per venue.
    venues: BTreeMap<VenueId, (f32, u64)>,
}

impl MeetHeat {
    /// Heat at a venue at an absolute in-game second (decayed, never stored).
    pub fn heat_at(&self, venue: VenueId, abs_s: u64, rules: &MeetHeatRules) -> f32 {
        self.venues.get(&venue).map_or(0.0, |(h, t)| {
            let hours = abs_s.saturating_sub(*t) as f32 / 3600.0;
            (h - rules.decay_per_hour * hours).max(0.0)
        })
    }

    /// Record a race starting at `venue`. Returns a patrol signal when the venue is hot.
    pub fn record_race(
        &mut self,
        venue: &Venue,
        abs_s: u64,
        rules: &MeetHeatRules,
    ) -> Option<RaceSignal> {
        let h = self.heat_at(venue.id, abs_s, rules) + rules.per_race;
        self.venues.insert(venue.id, (h, abs_s));
        (h >= rules.patrol_threshold).then_some(RaceSignal::PatrolAttracted {
            venue: venue.id,
            pos: venue.pos,
            heat: h,
        })
    }
}

impl ScheduleRules {
    /// Parse and validate TOML.
    pub fn from_toml(text: &str) -> Result<ScheduleRules, RaceDataError> {
        let t: ScheduleRules =
            toml::from_str(text).map_err(|e| RaceDataError::parse("schedule.toml", e))?;
        t.validate()?;
        Ok(t)
    }

    /// Check ranges and keys.
    pub fn validate(&self) -> Result<(), RaceDataError> {
        let bad = |what: &str, why: &str| Err(RaceDataError::invalid("schedule.toml", what, why));
        for (name, s) in [
            ("sanctioned", &self.sanctioned),
            ("night_meets", &self.night_meets),
            ("illegal_daytime", &self.illegal_daytime.slot),
        ] {
            if s.per_day[0] > s.per_day[1] {
                return bad(&format!("{name}.per_day"), "min > max");
            }
            if !(0.0 <= s.hours[0] && s.hours[0] < s.hours[1] && s.hours[1] <= 30.0) {
                return bad(&format!("{name}.hours"), "need 0 <= from < to <= 30");
            }
            if s.kinds.is_empty() || s.kinds.values().all(|w| *w == 0) {
                return bad(&format!("{name}.kinds"), "needs a positive weight");
            }
            for k in s.kinds.keys() {
                if RaceKind::from_key(k).is_none() {
                    return bad(&format!("{name}.kinds.{k}"), "unknown race kind");
                }
            }
            if s.lobby_open_min > s.spawn_lead_min {
                return bad(name, "lobby_open_min must be <= spawn_lead_min");
            }
        }
        let day_window = |h: [f32; 2]| h[0] >= 7.0 && h[1] <= 18.0;
        if !day_window(self.sanctioned.hours) || !day_window(self.illegal_daytime.slot.hours) {
            return bad(
                "hours",
                "sanctioned and illegal_daytime must start in Day (07-18)",
            );
        }
        if !(0.0..=1.0).contains(&self.illegal_daytime.chance_per_day) {
            return bad("illegal_daytime.chance_per_day", "must be in 0..=1");
        }
        for k in self.bracket_weights.keys() {
            if ClassBracket::from_letter(k).is_none() {
                return bad(&format!("bracket_weights.{k}"), "unknown bracket");
            }
        }
        if self.bracket_weights.values().all(|w| *w == 0) {
            return bad("bracket_weights", "needs a positive weight");
        }
        if self.circuit_laps[0] == 0 || self.circuit_laps[0] > self.circuit_laps[1] {
            return bad("circuit_laps", "need 1 <= min <= max");
        }
        let m = &self.meet_heat;
        if m.per_race <= 0.0 || m.decay_per_hour < 0.0 || m.patrol_threshold <= 0.0 {
            return bad("meet_heat", "per_race > 0, decay >= 0, threshold > 0");
        }
        if !(0.0..=1.0).contains(&m.hot_weight) {
            return bad("meet_heat.hot_weight", "must be in 0..=1");
        }
        Ok(())
    }
}

fn pick_weighted<'a>(rng: &mut Rng, items: &'a BTreeMap<String, u32>) -> &'a str {
    let total: u32 = items.values().sum();
    let mut r = rng.below(total.max(1));
    for (k, w) in items {
        if r < *w {
            return k;
        }
        r -= w;
    }
    items.keys().next().map_or("", |k| k.as_str())
}

fn range_u32(rng: &mut Rng, r: [u32; 2]) -> u32 {
    r[0] + rng.below(r[1] - r[0] + 1)
}

/// Generates calendars.
#[derive(Debug, Clone)]
pub struct Scheduler<'a> {
    /// Rules.
    pub rules: &'a ScheduleRules,
    /// Stake table.
    pub stakes: &'a StakeTable,
    /// Master seed of the world (calendars for different days are independent streams).
    pub seed: u64,
}

impl Scheduler<'_> {
    fn day_rng(&self, day: u32) -> Rng {
        Rng::new(self.seed ^ (day as u64 + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15))
    }

    /// Generate the calendar for `day`. `heat` is the meet heat at the start of the day (hot
    /// venues are picked less often); it is not modified — call [`MeetHeat::record_race`] when
    /// races actually start.
    pub fn generate_day(&self, day: u32, venues: &[Venue], heat: &MeetHeat) -> DayCalendar {
        let mut rng = self.day_rng(day);
        let mut events: Vec<ScheduledEvent> = Vec::new();
        let mut booked: Vec<(VenueId, u32)> = Vec::new();
        let r = self.rules;
        let gap = r.venue_gap_min * 60;

        // Sanctioned day events on closed circuits.
        let circuits: Vec<&Venue> = venues.iter().filter(|v| v.closed_circuit).collect();
        if !circuits.is_empty() {
            let n = range_u32(&mut rng, r.sanctioned.per_day);
            for _ in 0..n {
                let weights: Vec<u32> = circuits.iter().map(|_| 100).collect();
                if let Some(e) = self.make_event(
                    &mut rng,
                    day,
                    events.len(),
                    &r.sanctioned,
                    Legality::Sanctioned,
                    &circuits,
                    &weights,
                    &mut booked,
                    gap,
                ) {
                    events.push(e);
                }
            }
        }

        // Rare illegal daytime race.
        if !venues.is_empty() && rng.chance(r.illegal_daytime.chance_per_day) {
            let all: Vec<&Venue> = venues.iter().collect();
            let weights: Vec<u32> = all.iter().map(|_| 100).collect();
            if let Some(e) = self.make_event(
                &mut rng,
                day,
                events.len(),
                &r.illegal_daytime.slot,
                Legality::IllegalDaytime,
                &all,
                &weights,
                &mut booked,
                gap,
            ) {
                events.push(e);
            }
        }

        // Night meets; hot venues are down-weighted.
        if !venues.is_empty() {
            let all: Vec<&Venue> = venues.iter().collect();
            let night_start = day as u64 * DAY_S as u64 + (r.night_meets.hours[0] * 3600.0) as u64;
            let weights: Vec<u32> = all
                .iter()
                .map(|v| {
                    let hot = heat.heat_at(v.id, night_start, &r.meet_heat)
                        >= r.meet_heat.patrol_threshold;
                    if hot {
                        ((r.meet_heat.hot_weight * 100.0) as u32).max(1)
                    } else {
                        100
                    }
                })
                .collect();
            let n = range_u32(&mut rng, r.night_meets.per_day);
            for _ in 0..n {
                if let Some(e) = self.make_event(
                    &mut rng,
                    day,
                    events.len(),
                    &r.night_meets,
                    Legality::Street,
                    &all,
                    &weights,
                    &mut booked,
                    gap,
                ) {
                    events.push(e);
                }
            }
        }

        events.sort_by_key(|e| (e.start_s, e.id));
        DayCalendar { day, events }
    }

    #[allow(clippy::too_many_arguments)]
    fn make_event(
        &self,
        rng: &mut Rng,
        day: u32,
        n: usize,
        slot: &SlotRules,
        legality: Legality,
        venues: &[&Venue],
        weights: &[u32],
        booked: &mut Vec<(VenueId, u32)>,
        gap_s: u32,
    ) -> Option<ScheduledEvent> {
        let r = self.rules;
        let kind_key = pick_weighted(rng, &slot.kinds).to_string();
        let mut kind = RaceKind::from_key(&kind_key)?;
        if let RaceKind::Circuit { laps } = &mut kind {
            *laps = r.circuit_laps[0]
                + rng.below((r.circuit_laps[1] - r.circuit_laps[0]) as u32 + 1) as u8;
        }
        let bracket = ClassBracket::from_letter(pick_weighted(rng, &r.bracket_weights))?;
        // Start time on a 5-minute grid inside the window.
        let from = (slot.hours[0] * 60.0) as u32;
        let to = (slot.hours[1] * 60.0) as u32;
        let steps = ((to - from) / 5).max(1);
        // A few attempts to find a venue that is free at a time.
        for _ in 0..8 {
            let start_min = from + rng.below(steps) * 5;
            let start_s = start_min * 60;
            let total: u32 = weights.iter().sum();
            let mut pick = rng.below(total.max(1));
            let mut vi = 0;
            for (i, w) in weights.iter().enumerate() {
                if pick < *w {
                    vi = i;
                    break;
                }
                pick -= w;
            }
            let venue = venues[vi];
            let clash = booked
                .iter()
                .any(|(v, t)| *v == venue.id && t.abs_diff(start_s) < gap_s);
            if clash {
                continue;
            }
            booked.push((venue.id, start_s));
            let phase = TimeOfDay::from_hours(start_s as f32 / 3600.0).phase();
            let stake = self.stakes.stake(kind, phase, bracket, legality);
            let (traffic, marshals) = match (legality, kind) {
                (Legality::Sanctioned, _) => (TrafficMode::Suppressed, r.marshals),
                (_, RaceKind::Cannonball) => (TrafficMode::Heavy, 0),
                _ => (TrafficMode::Normal, 0),
            };
            let mut requirements = EntryRequirements::bracket(bracket);
            requirements.min_licence = slot.min_licence;
            requirements.min_rep = slot.min_rep;
            return Some(ScheduledEvent {
                id: EventId(day * 1000 + n as u32),
                day,
                kind,
                legality,
                venue: venue.id,
                bracket,
                phase,
                spawn_s: start_s.saturating_sub(slot.spawn_lead_min * 60),
                lobby_open_s: start_s.saturating_sub(slot.lobby_open_min * 60),
                start_s,
                stake,
                police_presence_override: stake.police,
                traffic,
                marshals,
                requirements,
            });
        }
        None
    }
}
