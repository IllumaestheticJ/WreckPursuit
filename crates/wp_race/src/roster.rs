//! The city's named street racers (`data/events/racers.toml`) and their persistent track
//! record.
//!
//! [`Roster`] is static data: who the regulars are, what they drive, how they drive
//! (`wp_opponent::DriverProfile`), where they hang out and which kind of course suits them.
//! [`RosterState`] is the save-game side: every racer's starts, wins, podiums, DNFs, recent
//! form and an Elo-like form rating that moves after every race, the player's included
//! (key [`PLAYER_KEY`]). The betting book ([`crate::betting`]) prices its odds from these
//! ratings, so a regular on a hot streak gets shorter odds the next night.
//!
//! [`field_for_event`] fills a scheduled night meet with regulars: the same faces turn up at
//! venues in their part of town, and the strongest only show once the player's licence tier is
//! high enough. Races the player skips can be resolved off screen ([`resolve_offscreen`]) so
//! the regulars' records keep moving.
//!
//! Everything is deterministic: integer ratings, `BTreeMap` storage, a seeded `wp_math::Rng`
//! for picks and off-screen results, and [`pow10`] built from IEEE-exact `+ − × ÷` only.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use wp_contracts::{EntityId, RacerStatus};
use wp_math::Rng;
use wp_opponent::DriverProfile;

use crate::error::RaceDataError;
use crate::result::RacerResult;
use crate::schedule::{ScheduledEvent, Venue};

/// Roster / record key of the player.
pub const PLAYER_KEY: &str = "player";

/// Form positions remembered per racer (most recent last).
pub const FORM_LEN: usize = 5;

/// Course types a racer can specialise in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Specialty {
    /// Hairpins and elevation (Hollins Drive, Ridge Road).
    Hill,
    /// Long straights and fast sweepers (Loop 9, the grid's highway).
    Highway,
    /// Junctions, traffic, short blocks.
    Downtown,
    /// No preference.
    Mixed,
}

impl Specialty {
    /// Lower-case key (as in the TOML).
    pub fn key(&self) -> &'static str {
        match self {
            Specialty::Hill => "hill",
            Specialty::Highway => "highway",
            Specialty::Downtown => "downtown",
            Specialty::Mixed => "mixed",
        }
    }
}

/// Licence tier at which a regular starts turning up at scheduled meets. The order matches
/// `data/events/progression.toml [[licences]]` (index 0 = Rookie).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    /// Licence index 0.
    Rookie,
    /// Licence index 1.
    Street,
    /// Licence index 2.
    Pro,
    /// Licence index 3.
    Elite,
    /// Licence index 4.
    Legend,
}

impl Tier {
    /// Licence tier index (`ProgressionTable::licence_for`).
    pub fn index(&self) -> u8 {
        *self as u8
    }
}

/// One named regular.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NpcRacer {
    /// Stable key (save files, bets).
    pub id: String,
    /// Display name.
    pub name: String,
    /// Street name, shown in quotes.
    pub nickname: String,
    /// Car id (`data/cars`).
    pub car: String,
    /// Part of town they haunt.
    pub home: String,
    /// Course type they are best on.
    pub specialty: Specialty,
    /// Lowest player licence tier at which they appear at scheduled meets.
    pub tier: Tier,
    /// Starting form rating (1000 = mid-pack).
    pub rating: i32,
    /// One-line personality.
    pub style: String,
    /// Driving personality.
    pub profile: DriverProfile,
}

impl NpcRacer {
    /// `Name "Nickname"` for menus.
    pub fn billing(&self) -> String {
        match self.name.split_once(' ') {
            Some((first, last)) => format!("{first} \"{}\" {last}", self.nickname),
            None => format!("{} \"{}\"", self.name, self.nickname),
        }
    }
}

/// `racers.toml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RosterFile {
    racer: Vec<NpcRacer>,
}

/// Every named regular, in file order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Roster {
    /// The racers.
    pub racers: Vec<NpcRacer>,
}

const FILE: &str = "racers.toml";

/// Starting rating of the player.
pub const PLAYER_START_RATING: i32 = 1000;

impl Roster {
    /// The repository's `data/events/racers.toml`, embedded at compile time.
    pub const EMBEDDED_TOML: &'static str = include_str!("../../../data/events/racers.toml");

    /// Parse and validate `racers.toml`.
    pub fn from_toml(text: &str) -> Result<Roster, RaceDataError> {
        let f: RosterFile = toml::from_str(text).map_err(|e| RaceDataError::parse(FILE, e))?;
        let r = Roster { racers: f.racer };
        r.validate()?;
        Ok(r)
    }

    /// The embedded roster (panics only if the shipped data is invalid, which the tests rule
    /// out).
    pub fn embedded() -> Roster {
        Roster::from_toml(Self::EMBEDDED_TOML).expect("data/events/racers.toml is valid")
    }

    fn validate(&self) -> Result<(), RaceDataError> {
        let bad = |what: &str, why: &str| Err(RaceDataError::invalid(FILE, what, why));
        if self.racers.len() < 6 {
            return bad("racer", "need at least 6 regulars to fill a meet");
        }
        for (i, r) in self.racers.iter().enumerate() {
            let what = format!("racer[{i}] {}", r.id);
            if r.id.is_empty() || r.id == PLAYER_KEY {
                return bad(&what, "id must be non-empty and not \"player\"");
            }
            if self.racers[..i].iter().any(|o| o.id == r.id) {
                return bad(&what, "duplicate id");
            }
            if r.name.is_empty() || r.car.is_empty() {
                return bad(&what, "name and car are required");
            }
            if !(400..=2000).contains(&r.rating) {
                return bad(&what, "rating must be 400..=2000");
            }
            let p = r.profile;
            for v in [p.skill, p.aggression, p.consistency, p.risk, p.awareness] {
                if !(0.0..=1.0).contains(&v) {
                    return bad(&what, "profile fields must be 0..=1");
                }
            }
        }
        Ok(())
    }

    /// The racer with this id.
    pub fn get(&self, id: &str) -> Option<&NpcRacer> {
        self.racers.iter().find(|r| r.id == id)
    }

    /// The racer with this display name.
    pub fn by_name(&self, name: &str) -> Option<&NpcRacer> {
        self.racers.iter().find(|r| r.name == name)
    }
}

/// Rating parameters (`betting.toml [rating]`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RatingConfig {
    /// Elo K factor per race (split over the field's pairings).
    pub k: i32,
    /// Rating bonus on a course that matches the racer's specialty.
    pub specialty_bonus: i32,
    /// Ratings never fall below this.
    pub floor: i32,
    /// Rating points per class-index point the player's car is above the field's median car.
    pub car_weight: f32,
    /// Cap on the car adjustment, ± rating points.
    pub car_cap: i32,
}

impl Default for RatingConfig {
    fn default() -> Self {
        Self {
            k: 32,
            specialty_bonus: 60,
            floor: 400,
            car_weight: 0.6,
            car_cap: 200,
        }
    }
}

impl RatingConfig {
    /// Rating adjustment for the player's car (class index `player_class`) against the field's
    /// cars (`field_classes`, any order): `car_weight` points per class-index point above the
    /// field's median, capped at `±car_cap`. NPC ratings already include their own car.
    pub fn car_adjust(&self, player_class: u16, field_classes: &[u16]) -> i32 {
        if field_classes.is_empty() {
            return 0;
        }
        let mut c = field_classes.to_vec();
        c.sort_unstable();
        let median = c[c.len() / 2] as f32;
        let raw = (player_class as f32 - median) * self.car_weight;
        (raw as i32).clamp(-self.car_cap, self.car_cap)
    }
}

/// One racer's persistent record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RacerRecord {
    /// Races started.
    pub starts: u32,
    /// Wins.
    pub wins: u32,
    /// Top-three finishes (wins included).
    pub podiums: u32,
    /// Did not finish / disqualified.
    pub dnfs: u32,
    /// Current form rating.
    pub rating: i32,
    /// Best rating reached.
    pub peak_rating: i32,
    /// Last [`FORM_LEN`] results, oldest first: finishing position, 0 = DNF/DQ.
    pub form: Vec<u8>,
}

impl RacerRecord {
    /// A fresh record at `rating`.
    pub fn new(rating: i32) -> Self {
        Self {
            starts: 0,
            wins: 0,
            podiums: 0,
            dnfs: 0,
            rating,
            peak_rating: rating,
            form: Vec::new(),
        }
    }

    /// Win rate 0..=1 (0 with no starts).
    pub fn win_rate(&self) -> f32 {
        if self.starts == 0 {
            0.0
        } else {
            self.wins as f32 / self.starts as f32
        }
    }

    /// Form as a short string, oldest first: `1`–`9` positions, `X` DNF, `-` not raced.
    pub fn form_string(&self) -> String {
        let mut s: String =
            std::iter::repeat_n('-', FORM_LEN.saturating_sub(self.form.len())).collect();
        for p in &self.form {
            s.push(match p {
                0 => 'X',
                1..=9 => char::from(b'0' + p),
                _ => '+',
            });
        }
        s
    }
}

/// One finisher of a race, for [`RosterState::record_race`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placing {
    /// Roster id or [`PLAYER_KEY`].
    pub key: String,
    /// Final position (1 = win), unique within the race.
    pub position: u8,
    /// Finished the race (DNF / DQ count as last behind every finisher).
    pub finished: bool,
}

impl Placing {
    /// Placings from judged results, given each racer entity's roster key (entities without
    /// a key are skipped: they are nobody's record).
    pub fn from_results(
        results: &[RacerResult],
        key_of: impl Fn(EntityId) -> Option<String>,
    ) -> Vec<Placing> {
        results
            .iter()
            .filter_map(|r| {
                key_of(r.racer).map(|key| Placing {
                    key,
                    position: r.position,
                    finished: r.status == RacerStatus::Finished,
                })
            })
            .collect()
    }
}

/// Every racer's record (the save-game side of the roster).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RosterState {
    /// Records by roster id ([`PLAYER_KEY`] for the player).
    pub records: BTreeMap<String, RacerRecord>,
    /// Races recorded so far.
    pub races: u32,
}

impl RosterState {
    /// Fresh records at each racer's starting rating, plus the player at
    /// [`PLAYER_START_RATING`].
    pub fn new(roster: &Roster) -> Self {
        let mut records: BTreeMap<String, RacerRecord> = roster
            .racers
            .iter()
            .map(|r| (r.id.clone(), RacerRecord::new(r.rating)))
            .collect();
        records.insert(PLAYER_KEY.into(), RacerRecord::new(PLAYER_START_RATING));
        Self { records, races: 0 }
    }

    /// Bring a loaded save up to date with the roster (racers added since get fresh records;
    /// racers removed keep theirs, harmlessly).
    pub fn sync(&mut self, roster: &Roster) {
        for r in &roster.racers {
            self.records
                .entry(r.id.clone())
                .or_insert_with(|| RacerRecord::new(r.rating));
        }
        self.records
            .entry(PLAYER_KEY.into())
            .or_insert_with(|| RacerRecord::new(PLAYER_START_RATING));
    }

    /// The record of `key` (`None` for unknown keys).
    pub fn get(&self, key: &str) -> Option<&RacerRecord> {
        self.records.get(key)
    }

    /// Current rating of `key` ([`PLAYER_START_RATING`] for unknown keys).
    pub fn rating(&self, key: &str) -> i32 {
        self.records
            .get(key)
            .map_or(PLAYER_START_RATING, |r| r.rating)
    }

    /// Record one race: starts, wins, podiums, DNFs, form, and a multi-player Elo update
    /// (every pair of racers is one game, the better final position wins it; `cfg.k` is
    /// spread over the `n − 1` games each racer plays). Unknown keys get a fresh record at
    /// [`PLAYER_START_RATING`]. Returns each racer's rating change, in input order.
    pub fn record_race(&mut self, placings: &[Placing], cfg: &RatingConfig) -> Vec<(String, i32)> {
        for p in placings {
            self.records
                .entry(p.key.clone())
                .or_insert_with(|| RacerRecord::new(PLAYER_START_RATING));
        }
        let ratings: Vec<i32> = placings.iter().map(|p| self.rating(&p.key)).collect();
        let n = placings.len();
        let mut deltas = Vec::with_capacity(n);
        for (i, p) in placings.iter().enumerate() {
            let mut sum = 0.0f32;
            for (j, q) in placings.iter().enumerate() {
                if i == j {
                    continue;
                }
                let expected = expected_score(ratings[i], ratings[j]);
                let actual = match (p.finished, q.finished) {
                    (true, false) => 1.0,
                    (false, true) => 0.0,
                    // Both finished: position decides. Both out: a draw.
                    (true, true) => {
                        if p.position < q.position {
                            1.0
                        } else {
                            0.0
                        }
                    }
                    (false, false) => 0.5,
                };
                sum += actual - expected;
            }
            let d = if n > 1 {
                round_half_away(cfg.k as f32 * sum / (n - 1) as f32)
            } else {
                0
            };
            deltas.push((p.key.clone(), d));
        }
        for (p, (_, d)) in placings.iter().zip(&deltas) {
            let rec = self.records.get_mut(&p.key).expect("inserted above");
            rec.starts += 1;
            if p.finished {
                if p.position == 1 {
                    rec.wins += 1;
                }
                if p.position <= 3 {
                    rec.podiums += 1;
                }
            } else {
                rec.dnfs += 1;
            }
            rec.form.push(if p.finished { p.position } else { 0 });
            if rec.form.len() > FORM_LEN {
                rec.form.remove(0);
            }
            rec.rating = (rec.rating + d).max(cfg.floor);
            rec.peak_rating = rec.peak_rating.max(rec.rating);
        }
        self.races += 1;
        deltas
    }

    /// Serialise for a save file.
    pub fn to_json(&self) -> String {
        serde_json_lite::to_string(self)
    }

    /// Load from [`RosterState::to_json`] output.
    pub fn from_json(text: &str) -> Result<RosterState, String> {
        serde_json_lite::from_str(text)
    }
}

// wp_race has no serde_json dependency (only a dev-dependency); a save goes through TOML,
// which every target already parses.
mod serde_json_lite {
    use super::RosterState;

    pub fn to_string(s: &RosterState) -> String {
        toml::to_string(s).expect("RosterState serialises")
    }

    pub fn from_str(text: &str) -> Result<RosterState, String> {
        toml::from_str(text).map_err(|e| e.to_string())
    }
}

/// Elo expected score of a racer rated `a` against one rated `b`.
pub fn expected_score(a: i32, b: i32) -> f32 {
    1.0 / (1.0 + pow10((b - a) as f32 / 400.0))
}

/// Round to the nearest integer, halves away from zero, without `f32::round` platform
/// differences (`round` is exact everywhere, but keep the rule explicit).
fn round_half_away(x: f32) -> i32 {
    if x >= 0.0 {
        (x + 0.5) as i32
    } else {
        -((-x + 0.5) as i32)
    }
}

/// `10^x` from `+ − × ÷` only (IEEE-exact on every target, unlike `powf`): range reduction to
/// `2^k · 2^f`, `f ∈ [0, 1)`, and a 12-term Taylor series for `e^(f ln 2)` (error < 1e-7).
/// Accurate for |x| ≤ 30, which covers any rating gap.
pub fn pow10(x: f32) -> f32 {
    const LOG2_10: f32 = std::f32::consts::LOG2_10;
    const LN_2: f32 = std::f32::consts::LN_2;
    let y = (x * LOG2_10).clamp(-100.0, 100.0);
    // floor without platform calls: truncation toward zero, then fix negatives.
    let mut k = y as i64;
    if (k as f32) > y {
        k -= 1;
    }
    let f = (y - k as f32) * LN_2;
    let mut term = 1.0f32;
    let mut sum = 1.0f32;
    for i in 1..=12 {
        term *= f / i as f32;
        sum += term;
    }
    let (mul, steps) = if k >= 0 { (2.0, k) } else { (0.5, -k) };
    for _ in 0..steps {
        sum *= mul;
    }
    sum
}

/// Course effect on a racer's rating: `bonus` on their specialty course, nothing otherwise.
pub fn specialty_bonus(r: &NpcRacer, course: Specialty, cfg: &RatingConfig) -> i32 {
    if r.specialty == course && course != Specialty::Mixed {
        cfg.specialty_bonus
    } else {
        0
    }
}

/// Who is asking for a meet field ([`field_for_event`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MeetCall<'a> {
    /// District label of the venue (`wp_world` zone name), matched against racers' `home`.
    pub district_name: Option<&'a str>,
    /// The player's licence tier index (`ProgressionTable::licence_for`).
    pub player_licence: u8,
    /// Regulars wanted (3–5 is typical).
    pub size: usize,
    /// World seed.
    pub seed: u64,
}

/// Pick regulars for a scheduled meet at `venue`: only racers whose tier the player's licence
/// has reached, weighted towards those who haunt the venue (`home` found in the venue name or
/// the district label) and towards ratings close to the player's. Deterministic in
/// `(call.seed, event id)`.
pub fn field_for_event<'a>(
    roster: &'a Roster,
    state: &RosterState,
    event: &ScheduledEvent,
    venue: &Venue,
    call: &MeetCall<'_>,
) -> Vec<&'a NpcRacer> {
    let (district_name, player_licence, n, seed) = (
        call.district_name,
        call.player_licence,
        call.size,
        call.seed,
    );
    let mut rng = Rng::new(seed ^ (event.id.0 as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15));
    let player = state.rating(PLAYER_KEY);
    let mut pool: Vec<(&NpcRacer, u32)> = roster
        .racers
        .iter()
        .filter(|r| r.tier.index() <= player_licence)
        .map(|r| {
            let local = venue.name.contains(r.home.as_str())
                || district_name.is_some_and(|d| d.contains(r.home.as_str()));
            let gap = (state.rating(&r.id) - player).unsigned_abs();
            // 4 for a near match, down to 1 for 300+ points apart; ×3 on home turf.
            let closeness = 4u32.saturating_sub(gap / 100).max(1);
            (r, closeness * if local { 3 } else { 1 })
        })
        .collect();
    let mut out = Vec::with_capacity(n);
    while out.len() < n && !pool.is_empty() {
        let total: u32 = pool.iter().map(|(_, w)| *w).sum();
        let mut pick = rng.below(total);
        let idx = pool
            .iter()
            .position(|(_, w)| {
                if pick < *w {
                    true
                } else {
                    pick -= *w;
                    false
                }
            })
            .expect("pick < total");
        out.push(pool.remove(idx).0);
    }
    out
}

/// Draw a finishing order for a race nobody watched (Plackett–Luce on rating strengths: the
/// winner is drawn with probability ∝ `10^(rating/400)`, then second from the rest, and so
/// on). Each entry is `(key, rating incl. course bonus)`; `dnf_chance` is each racer's
/// per-race chance of not finishing (risky drivers bin it more). Returns placings ready for
/// [`RosterState::record_race`].
pub fn resolve_offscreen(entrants: &[(String, i32, f32)], seed: u64) -> Vec<Placing> {
    let mut rng = Rng::new(seed);
    let mut left: Vec<(String, f32, bool)> = entrants
        .iter()
        .map(|(k, r, dnf)| (k.clone(), pow10(*r as f32 / 400.0), !rng.chance(*dnf)))
        .collect();
    // Finishers are ordered by strength draws; non-finishers fill the back in entry order.
    let mut out = Vec::with_capacity(left.len());
    let mut pos = 1u8;
    while left.iter().any(|e| e.2) {
        let total: f32 = left.iter().filter(|e| e.2).map(|e| e.1).sum();
        let mut x = rng.next_f32() as f32 * total;
        let mut idx = None;
        for (i, e) in left.iter().enumerate() {
            if !e.2 {
                continue;
            }
            idx = Some(i);
            if x < e.1 {
                break;
            }
            x -= e.1;
        }
        let e = left.remove(idx.expect("a finisher is left"));
        out.push(Placing {
            key: e.0,
            position: pos,
            finished: true,
        });
        pos += 1;
    }
    for e in left {
        out.push(Placing {
            key: e.0,
            position: pos,
            finished: false,
        });
        pos += 1;
    }
    out
}

/// A racer's per-race DNF chance for [`resolve_offscreen`]: 3 % for a careful, consistent
/// driver, up to ~18 % for a reckless, erratic one.
pub fn offscreen_dnf_chance(p: &DriverProfile) -> f32 {
    0.03 + 0.10 * p.risk * (1.0 - p.consistency) * 2.0 + 0.05 * (1.0 - p.awareness)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_roster_is_valid_and_varied() {
        let r = Roster::embedded();
        assert!(r.racers.len() >= 12);
        for s in [Specialty::Hill, Specialty::Highway, Specialty::Downtown] {
            assert!(
                r.racers.iter().filter(|x| x.specialty == s).count() >= 2,
                "{s:?}"
            );
        }
        assert!(r.racers.iter().any(|x| x.tier >= Tier::Elite));
        assert_eq!(
            r.get("mara").unwrap().billing(),
            "Mara \"Veteran\" Lindqvist"
        );
    }

    #[test]
    fn pow10_matches_reference() {
        for x in [-3.0, -1.25, -0.5, 0.0, 0.1, 0.5, 1.0, 2.5, 4.0] {
            let want = 10f32.powf(x);
            assert!(((pow10(x) - want) / want).abs() < 1e-5, "{x}");
        }
        assert_eq!(expected_score(1000, 1000), 0.5);
        assert!((expected_score(1400, 1000) - 0.909_090_9).abs() < 1e-5);
    }

    #[test]
    fn elo_rewards_upsets_and_is_zero_sum_ish() {
        let roster = Roster::embedded();
        let mut st = RosterState::new(&roster);
        let cfg = RatingConfig::default();
        let pl = |k: &str, p: u8, f: bool| Placing {
            key: k.into(),
            position: p,
            finished: f,
        };
        // Tito (880) beats Nina (1300) and Mara (1180): a big upset.
        let d = st.record_race(
            &[
                pl("tito", 1, true),
                pl("nina", 2, true),
                pl("mara", 3, true),
            ],
            &cfg,
        );
        assert!(d[0].1 > 20, "{d:?}");
        assert!(d[1].1 < 0 && d[2].1 < 0);
        let sum: i32 = d.iter().map(|x| x.1).sum();
        assert!(sum.abs() <= 2, "{d:?}");
        let t = st.get("tito").unwrap();
        assert_eq!((t.starts, t.wins, t.podiums), (1, 1, 1));
        assert_eq!(t.form_string(), "----1");
        // A DNF loses to every finisher.
        let d = st.record_race(&[pl("bex", 2, false), pl("dez", 1, true)], &cfg);
        assert!(d[0].1 < 0 && d[1].1 > 0);
        assert_eq!(st.get("bex").unwrap().form_string(), "----X");
        // Round-trips through a save.
        let back = RosterState::from_json(&st.to_json()).unwrap();
        assert_eq!(back, st);
    }

    #[test]
    fn form_keeps_the_last_five() {
        let roster = Roster::embedded();
        let mut st = RosterState::new(&roster);
        for i in 0..8u8 {
            st.record_race(
                &[
                    Placing {
                        key: "vic".into(),
                        position: 1 + i % 3,
                        finished: true,
                    },
                    Placing {
                        key: "juno".into(),
                        position: 4,
                        finished: true,
                    },
                ],
                &RatingConfig::default(),
            );
        }
        let v = st.get("vic").unwrap();
        assert_eq!(v.form.len(), FORM_LEN);
        assert_eq!(v.starts, 8);
    }

    #[test]
    fn offscreen_results_favour_the_strong_and_are_deterministic() {
        let field = vec![
            ("nina".to_string(), 1300, 0.0),
            ("tito".to_string(), 880, 0.0),
            ("bex".to_string(), 820, 0.5),
        ];
        let a = resolve_offscreen(&field, 7);
        assert_eq!(a, resolve_offscreen(&field, 7));
        let mut nina_wins = 0;
        for s in 0..400 {
            let r = resolve_offscreen(&field, s);
            assert_eq!(r.len(), 3);
            let mut pos: Vec<u8> = r.iter().map(|p| p.position).collect();
            pos.sort();
            assert_eq!(pos, vec![1, 2, 3]);
            if r[0].key == "nina" {
                nina_wins += 1;
            }
        }
        // Strength share ≈ 0.86 of the win against these two.
        assert!((300..390).contains(&nina_wins), "{nina_wins}");
    }

    #[test]
    fn meet_fields_respect_tiers_and_favour_locals() {
        use crate::kind::{ClassBracket, EventId, Legality, RaceKind, VenueId};
        let roster = Roster::embedded();
        let state = RosterState::new(&roster);
        let data = crate::RaceData::embedded();
        let event = ScheduledEvent {
            id: EventId(7),
            day: 0,
            kind: RaceKind::Sprint,
            legality: Legality::Street,
            venue: VenueId(0),
            bracket: ClassBracket::D,
            phase: wp_contracts::DayPhase::Night,
            spawn_s: 0,
            lobby_open_s: 0,
            start_s: 0,
            stake: data.stakes.stake(
                RaceKind::Sprint,
                wp_contracts::DayPhase::Night,
                ClassBracket::D,
                Legality::Street,
            ),
            police_presence_override: crate::PolicePresence::Normal,
            traffic: crate::schedule::TrafficMode::Normal,
            marshals: 0,
            requirements: crate::EntryRequirements::bracket(ClassBracket::D),
        };
        let venue = Venue {
            id: VenueId(0),
            name: "Harbor Boulevard drag".into(),
            pos: [0.0; 3],
            district: None,
            closed_circuit: false,
        };
        let call = |licence: u8, seed: u64| MeetCall {
            district_name: None,
            player_licence: licence,
            size: 4,
            seed,
        };
        let a = field_for_event(&roster, &state, &event, &venue, &call(0, 3));
        assert_eq!(
            a,
            field_for_event(&roster, &state, &event, &venue, &call(0, 3))
        );
        assert_eq!(a.len(), 4);
        let mut harbor = 0;
        for seed in 0..200 {
            let f = field_for_event(&roster, &state, &event, &venue, &call(0, seed));
            // A rookie never meets the pros or the elite.
            assert!(f.iter().all(|r| r.tier == Tier::Rookie), "{seed}");
            let mut ids: Vec<&str> = f.iter().map(|r| r.id.as_str()).collect();
            ids.sort();
            ids.dedup();
            assert_eq!(ids.len(), 4);
            harbor += f.iter().filter(|r| r.home == "Harbor").count();
        }
        // Two Harbor regulars out of seven rookies: well above their 2/7 share of seats.
        assert!(harbor as f32 / 800.0 > 0.4, "{harbor}");
        let legend = field_for_event(&roster, &state, &event, &venue, &call(4, 1));
        assert_eq!(legend.len(), 4);
    }

    #[test]
    fn dnf_chance_is_sane() {
        for r in &Roster::embedded().racers {
            let c = offscreen_dnf_chance(&r.profile);
            assert!((0.03..0.25).contains(&c), "{} {c}", r.id);
        }
    }
}
