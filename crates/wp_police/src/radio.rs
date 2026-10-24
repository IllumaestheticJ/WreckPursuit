//! Radio pacing: decides which police radio lines are spoken, and when.
//!
//! Every dispatch decision, state change and unit callout ([`crate::chatter`]) *raises* a
//! line ([`RadioDirector::raise`]); the director holds it and, once per police step
//! ([`RadioDirector::flush`]), decides what is actually transmitted (radio event + scanner
//! log). Rules, tuned in `data/police/radio.toml`:
//!
//! - **Priority tiers** (owner playtest row 144, "filtering between important and unimportant
//!   information"), high to low ([`RadioTier`]):
//!
//!   | Tier | Content | Pacing |
//!   |---|---|---|
//!   | 1 [`RadioTier::Actionable`] | roadblock / spikes planned (where, how far), helicopter launching / arriving / losing the racer, a unit intercepting ahead | no gap; cuts any lower line on air |
//!   | 2 [`RadioTier::Escalation`] | pursuit start, wanted level up / down, a tougher department taking over, rams authorised, pursuit end, captain orders | no gap; cuts tiers 3–5 on air |
//!   | 3 [`RadioTier::Impact`] | rams, PITs, units hit / wrecked, civilian crashes caused by the chase | `impact_gap_s` only; short life (`impact_stale_s`) |
//!   | 4 [`RadioTier::Investigation`] | calls, searching, lost visual / last seen, eyes on again, bumper, box-in | `global_gap_s` by level, per-unit gap |
//!   | 5 [`RadioTier::Routine`] | resuming patrol, clear, out of service, unrelated incidents | `routine_gap_s` by level (thins out as heat rises), `routine_quiet_s` after any tier 1–3 line, dropped at once while a higher line waits |
//!
//! - **One transmission at a time** (playtest row 79). A spoken line keeps the channel busy for
//!   its estimated length (`words / words_per_s + line_pad_s`) plus `min_gap_s` of silence
//!   (`impact_min_gap_s` after a shouted tier 3 line; a tier 1 line waits for the
//!   transmission only, not the silence).
//!   The only way two lines meet is a *cut*: a waiting tier 1 line cuts any lower line on air,
//!   a tier 2 line cuts tiers 3–5, and emergency traffic ([`RadioCue::breaks_in`]: an officer
//!   rammed, a captain's order) cuts any line below tier 1 that is not itself a break-in; only
//!   tier 1 cuts emergency traffic. Waiting lines form a priority queue: tier 1, then
//!   emergency traffic, then tier, then the cue's `prio`, then the older line.
//! - **Per-unit gap by wanted level** for tiers 4–5: a unit says at most one such line per
//!   `unit_gap_s[level - 1]` (L1 / calm: 30 s, the row-6 value).
//! - **Collapse.** Lines raised with the same group key within `collapse_window_s` become one
//!   line ("Units 4 and 5 resuming patrol.").
//! - **Earshot.** Routine chatter needs a wanted racer, or the speaking unit within
//!   `earshot_m` of a racer (dispatch's own routine lines need a wanted racer).
//!
//! Deployment lines ("Unit 9 in pursuit", "copy, sending unit 3 of 4", "moving to intercept"
//! behind the racer) are filtered where they are raised (`dispatch.rs`, [`crate::chatter`]),
//! not here: none at L0–L1 except one "units responding" summary when heat first rises, and at
//! L2+ only the actionable ones (tier 1).
//!
//! Deterministic: sim time only, no RNG, fixed iteration order. Nothing here feeds back into
//! the simulation or the state hash.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use wp_contracts::EntityId;

use crate::events::UnitId;
use crate::time::SimTime;

/// How much a radio line matters (owner playtest row 144). Ordered low → high, so
/// `Actionable` is the greatest; [`RadioTier::number`] gives the brief's tier number (1 = most
/// important).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum RadioTier {
    /// Tier 5: acknowledgements, resuming patrol, clear and available, out of service,
    /// unrelated incidents. Only on a quiet channel; dropped first; thins out as heat rises.
    Routine,
    /// Tier 4: calls, searching, lost visual / last seen, eyes on again, bumper and box-in
    /// calls. Only when the channel is free (row-79 gaps). Logs from before row 144 said
    /// `Tactical`.
    #[serde(alias = "Tactical")]
    Investigation,
    /// Tier 3: rams, PITs, units hit or wrecked, civilian crashes caused by the chase. Shouted;
    /// short gaps.
    Impact,
    /// Tier 2: pursuit start and end, wanted level up or down, a tougher department taking
    /// over, rams authorised, captain orders. Pre-empts tiers 3–5. Logs from before row 144
    /// said `Critical`.
    #[serde(alias = "Critical")]
    Escalation,
    /// Tier 1: roadblock / spikes planned, helicopter launching / arriving / losing the racer, a
    /// unit intercepting ahead. Always gets through: cuts any lower line.
    Actionable,
}

impl RadioTier {
    /// Every tier, most important first.
    pub const ALL: [RadioTier; 5] = [
        RadioTier::Actionable,
        RadioTier::Escalation,
        RadioTier::Impact,
        RadioTier::Investigation,
        RadioTier::Routine,
    ];

    /// The brief's tier number: 1 = actionable … 5 = routine.
    pub fn number(self) -> u8 {
        match self {
            RadioTier::Actionable => 1,
            RadioTier::Escalation => 2,
            RadioTier::Impact => 3,
            RadioTier::Investigation => 4,
            RadioTier::Routine => 5,
        }
    }

    /// Tiers 1–3: what the "Radio: Important only" setting keeps.
    pub fn important(self) -> bool {
        self >= RadioTier::Impact
    }

    /// Whether a waiting line of this tier may cut a line of tier `on_air` (tier 1 cuts any
    /// lower line, tier 2 cuts tiers 3–5).
    pub fn cuts(self, on_air: RadioTier) -> bool {
        self >= RadioTier::Escalation && self > on_air
    }
}

/// Serde default for logs written before tiers were recorded.
pub fn default_tier() -> RadioTier {
    RadioTier::Investigation
}

/// Collapse rule for a line: lines raised close together with the same `key` are spoken once,
/// as `text` with `{units}` (call signs, e.g. "Units 4 and 5") and `{count}` filled in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RadioGroup {
    /// Lines with equal keys merge.
    pub key: String,
    /// Text used when two or more lines merged.
    pub text: String,
}

/// A line raised by dispatch logic or a unit callout.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RadioCue {
    /// Priority tier.
    pub tier: RadioTier,
    /// Order inside a tier (higher first), e.g. a unit's backup request before dispatch's
    /// "copy". Default 0.
    pub prio: u8,
    /// Own staleness limit, s (`None` = the tier's: `stale_s` or `stale_critical_s`). Short
    /// for callouts that are only true for a moment ("I'm on his bumper").
    pub ttl_s: Option<f32>,
    /// Speaking unit (`None` = dispatch / captain).
    pub unit: Option<UnitId>,
    /// Unit call sign for collapsed lines ("Unit 4").
    pub callsign: Option<String>,
    /// Text when spoken alone.
    pub text: String,
    /// Collapse rule.
    pub group: Option<RadioGroup>,
    /// Supersede slot: a newer line with the same slot replaces a held (unspoken) one, e.g.
    /// "now level 4" replaces a queued "now level 3" for the same racer.
    pub slot: Option<String>,
    /// Emergency traffic: cut the transmission on air and go out at once (an officer rammed,
    /// a captain's order). Never cuts another break-in.
    pub break_in: bool,
    /// The racer the line is about (`None` = nobody in particular). Lines about a racer who is
    /// not a listener (see [`RadioContext::listeners`]) are only heard when that racer is near
    /// a listener. Filled in by `PoliceSystem::radio` when the caller leaves it empty.
    #[serde(default)]
    pub racer: Option<EntityId>,
    /// Where it happens (ground position, m), when that matters more than the speaker's own
    /// position: a crash, a dispatch line about a place.
    #[serde(default)]
    pub at: Option<[f32; 2]>,
}

impl RadioCue {
    /// A line with no collapse rule.
    pub fn new(tier: RadioTier, unit: Option<UnitId>, text: String) -> Self {
        Self {
            tier,
            prio: 0,
            ttl_s: None,
            unit,
            callsign: None,
            text,
            group: None,
            slot: None,
            break_in: false,
            racer: None,
            at: None,
        }
    }

    /// The racer this line is about.
    pub fn about(mut self, racer: EntityId) -> Self {
        self.racer = Some(racer);
        self
    }

    /// Where it happens, m.
    pub fn at(mut self, pos: [f32; 2]) -> Self {
        self.at = Some(pos);
        self
    }

    /// Add a collapse rule (`callsign` names this unit in the collapsed text).
    pub fn grouped(mut self, callsign: Option<String>, key: String, text: String) -> Self {
        self.callsign = callsign;
        self.group = Some(RadioGroup { key, text });
        self
    }

    /// Order inside the tier (higher first).
    pub fn prio(mut self, prio: u8) -> Self {
        self.prio = prio;
        self
    }

    /// Replace any held line with the same `slot` (it is out of date).
    pub fn supersedes(mut self, slot: String) -> Self {
        self.slot = Some(slot);
        self
    }

    /// Emergency traffic: cut whatever is on air (see [`RadioCue::break_in`]).
    pub fn breaks_in(mut self) -> Self {
        self.break_in = true;
        self
    }

    /// Drop the line if it could not be spoken within `s` seconds.
    pub fn ttl(mut self, s: f32) -> Self {
        self.ttl_s = Some(s);
        self
    }
}

fn d_min_gap() -> f32 {
    2.5
}
fn d_words_per_s() -> f32 {
    2.6
}
fn d_line_pad() -> f32 {
    0.7
}
fn d_stale_critical() -> f32 {
    15.0
}
fn d_impact_gap() -> f32 {
    0.4
}
fn d_impact_stale() -> f32 {
    4.0
}
fn d_impact_min_gap() -> f32 {
    0.8
}
fn d_routine_gap() -> [f32; 5] {
    [10.0, 14.0, 20.0, 30.0, 45.0]
}
fn d_routine_quiet() -> f32 {
    6.0
}
fn d_relevant() -> f32 {
    300.0
}
fn d_impact_m() -> f32 {
    250.0
}
fn d_max_per_min() -> [f32; 3] {
    [2.0, 4.0, 6.0]
}
fn d_true() -> bool {
    true
}

fn d_takeover() -> [String; 5] {
    [
        String::new(),
        String::new(),
        "Highway Patrol is taking over the pursuit.".into(),
        "Heavy units authorised.".into(),
        "Tactical units authorised, all units, stop him by any means.".into(),
    ]
}

/// Impact severity thresholds (`[impact]` in `data/police/radio.toml`, row 144), see
/// [`crate::chatter::impact_severity`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ImpactConfig {
    /// Collision energy for a "hit" line, J (below: a tap, no line).
    pub hit_j: f32,
    /// Collision energy for a "heavy" line, J.
    pub heavy_j: f32,
    /// Unit health at or below this makes any real hit at least "heavy", 0..1.
    pub heavy_health: f32,
    /// Mass for the energy of a contact without an impulse (headless proximity contacts), kg.
    pub nominal_mass_kg: f32,
    /// One impact line per unit per this long, unless a worse one follows, s.
    pub unit_gap_s: f32,
    /// One civilian-crash line per racer per this long, s.
    pub civilian_gap_s: f32,
}

impl Default for ImpactConfig {
    fn default() -> Self {
        Self {
            hit_j: 6_000.0,
            heavy_j: 30_000.0,
            heavy_health: 0.45,
            nominal_mass_kg: 1_500.0,
            unit_gap_s: 3.0,
            civilian_gap_s: 8.0,
        }
    }
}

/// Contents of `data/police/radio.toml`. All times are sim seconds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RadioConfig {
    /// Gap after any spoken line before a tier 4 (investigation) line, by the highest wanted
    /// level of any racer (L1..L5), s. L1 is the playtest row-6 value.
    pub global_gap_s: [f32; 5],
    /// Gap before a tier 4–5 line while no racer is wanted, s.
    pub global_gap_calm_s: f32,
    /// Gap after any spoken line before a tier 5 (routine) line, by wanted level (L1..L5), s.
    /// Grows with heat: routine chatter thins out as the chase heats up (row 144).
    #[serde(default = "d_routine_gap")]
    pub routine_gap_s: [f32; 5],
    /// No tier 5 line within this long after a tier 1–3 line, s.
    #[serde(default = "d_routine_quiet")]
    pub routine_quiet_s: f32,
    /// Gap after any spoken line before a tier 3 (impact) line, s: short, so a big wreck can
    /// stack the victim's line and another unit's reaction.
    #[serde(default = "d_impact_gap")]
    pub impact_gap_s: f32,
    /// Gap between one unit's tier 4–5 lines, by wanted level (L1..L5; nobody wanted uses
    /// L1), s.
    pub unit_gap_s: [f32; 5],
    /// Grouped lines wait this long for duplicates from other units to merge, s.
    pub collapse_window_s: f32,
    /// Held tier 4–5 lines older than this are dropped, s.
    pub stale_s: f32,
    /// Held tier 1–2 lines older than this are dropped, s.
    #[serde(default = "d_stale_critical")]
    pub stale_critical_s: f32,
    /// Held tier 3 lines older than this are dropped (an impact is only news for a moment), s.
    #[serde(default = "d_impact_stale")]
    pub impact_stale_s: f32,
    /// Silence after a tier 3 transmission (instead of `min_gap_s`): shouted traffic comes
    /// thick and fast, s.
    #[serde(default = "d_impact_min_gap")]
    pub impact_min_gap_s: f32,
    /// Routine chatter with nobody wanted plays only within this distance of a racer, m.
    pub earshot_m: f32,
    /// Silence after a transmission ends before the next one starts, s.
    #[serde(default = "d_min_gap")]
    pub min_gap_s: f32,
    /// Speaking rate for transmission lengths, words/s (= `wp_audio`'s scanner
    /// `words_per_s`, so spoken lines never overlap).
    #[serde(default = "d_words_per_s")]
    pub words_per_s: f32,
    /// Key-up / squelch time added to every transmission, s (= `wp_audio` `line_pad_s`).
    #[serde(default = "d_line_pad")]
    pub line_pad_s: f32,
    /// Impact severity thresholds (row 144).
    #[serde(default)]
    pub impact: ImpactConfig,
    /// "Tougher department" sentence added to dispatch's level summary when a pursued racer
    /// reaches level 1..5 (`""` = none). Fiction over the real escalation (more units, harder
    /// tactics at higher levels, `data/police/pursuit.toml`); the rams sentence comes from the
    /// pursuit config's ram levels.
    #[serde(default = "d_takeover")]
    pub takeover: [String; 5],
    /// A line about a racer who is not a listener is heard only while that racer (or the
    /// place, or the speaking unit) is within this distance of a listener, m (owner, 2026-10-23:
    /// "police chatter about AI cars on the other side of the map should not pollute the
    /// radio"). Only applies once listeners are set.
    #[serde(default = "d_relevant")]
    pub relevant_m: f32,
    /// Tier 3 (impact) lines are heard only when the speaking unit (or the place) is within
    /// this distance of a listener, m: a unit crashing a kilometre away is not news to the
    /// driver. Only applies once listeners are set.
    #[serde(default = "d_impact_m")]
    pub impact_m: f32,
    /// At most this many lines per minute (any 60 s window) may be spoken before a tier 3–5
    /// line is held back, by the listeners' state: [calm, wanted or searching, in pursuit].
    /// Tiers 1–2 always get through (and count).
    #[serde(default = "d_max_per_min")]
    pub max_lines_per_min: [f32; 3],
    /// Speak tier 5 (routine) lines at all ("clear and available", "resuming patrol",
    /// unrelated incidents). `data/police/radio.toml` switches them off (2026-10-23: no
    /// gameplay value); the serde default keeps them for older tables.
    #[serde(default = "d_true")]
    pub routine: bool,
}

impl RadioConfig {
    /// Estimated length of a transmission of `text`, s.
    pub fn line_s(&self, text: &str) -> f32 {
        let words = text.split_whitespace().count().max(1) as f32;
        words / self.words_per_s.max(0.5) + self.line_pad_s
    }

    /// Tier 4 gap after any line at the highest wanted `level` (0 = nobody), s.
    pub fn global_gap(&self, level: u8) -> f32 {
        match level {
            0 => self.global_gap_calm_s,
            l => self.global_gap_s[(l as usize - 1).min(4)],
        }
    }

    /// Tier 5 gap after any line at the highest wanted `level` (0 = nobody), s.
    pub fn routine_gap(&self, level: u8) -> f32 {
        match level {
            0 => self.global_gap_calm_s,
            l => self.routine_gap_s[(l as usize - 1).min(4)],
        }
    }

    /// Silence after a transmission of `tier` before the next one, s.
    pub fn after_line(&self, tier: RadioTier) -> f32 {
        if tier == RadioTier::Impact {
            self.impact_min_gap_s.min(self.min_gap_s)
        } else {
            self.min_gap_s
        }
    }

    /// Gap after any line before a line of `tier` at wanted `level`, s.
    pub fn gap(&self, tier: RadioTier, level: u8) -> f32 {
        match tier {
            RadioTier::Actionable | RadioTier::Escalation => 0.0,
            RadioTier::Impact => self.impact_gap_s,
            RadioTier::Investigation => self.global_gap(level),
            RadioTier::Routine => self.routine_gap(level),
        }
    }

    /// How long a held line of `tier` waits before it is dropped (unless the cue sets its own
    /// life), s.
    pub fn stale(&self, tier: RadioTier) -> f32 {
        match tier {
            RadioTier::Actionable | RadioTier::Escalation => self.stale_critical_s,
            RadioTier::Impact => self.impact_stale_s,
            RadioTier::Investigation | RadioTier::Routine => self.stale_s,
        }
    }

    /// Per-unit gap at the highest wanted `level` (0 = nobody, uses L1), s.
    pub fn unit_gap(&self, level: u8) -> f32 {
        self.unit_gap_s[(level.max(1) as usize - 1).min(4)]
    }

    /// Line budget per minute for tiers 3–5 in listener `state`.
    pub fn max_per_min(&self, state: ListenerState) -> f32 {
        self.max_lines_per_min[state as usize]
    }
}

/// What the listeners (the players) are going through, for the line budget
/// ([`RadioConfig::max_lines_per_min`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum ListenerState {
    /// Nobody listening is wanted.
    #[default]
    Calm = 0,
    /// Wanted or searched for, not in an active pursuit.
    Wanted = 1,
    /// In an active pursuit.
    Pursuit = 2,
}

/// Counters for tuning and tests.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RadioStats {
    /// Lines raised by dispatch logic and callouts.
    pub raised: u32,
    /// Lines spoken (after collapsing).
    pub spoken: u32,
    /// Raised lines merged into another spoken line.
    pub collapsed: u32,
    /// Raised lines never spoken (stale, out of earshot, unit cooldown).
    pub dropped: u32,
    /// Spoken lines per tier, index = tier number − 1 (0 = actionable … 4 = routine).
    pub spoken_by_tier: [u32; 5],
    /// Lines that cut a transmission on air.
    pub cuts: u32,
    /// Raised lines dropped because they did not concern any listener (far away, or another
    /// racer's business). Also counted in `dropped`.
    #[serde(default)]
    pub irrelevant: u32,
}

/// What the director needs to know about the moment of a flush.
pub struct RadioContext<'a> {
    /// Current sim time.
    pub now: SimTime,
    /// Highest wanted level of any listener (0 = nobody wanted, 1..=5). Without listeners:
    /// of any racer.
    pub level: u8,
    /// What the listeners are going through (line budget).
    pub state: ListenerState,
    /// Ground positions of the listeners (without listeners: of every racer), m.
    pub racers: &'a [[f32; 2]],
    /// The racers whose radio this is (the players). `None`: everyone listens, no line is
    /// dropped for relevance (headless harness, tests).
    pub listeners: Option<&'a [EntityId]>,
    /// Ground position of a racer, m (`None` if unknown).
    pub racer_pos: &'a dyn Fn(EntityId) -> Option<[f32; 2]>,
    /// Ground position of a unit, m (`None` if gone).
    pub unit_pos: &'a dyn Fn(UnitId) -> Option<[f32; 2]>,
}

impl RadioContext<'_> {
    /// Whether `p` lies within `r` metres of a listener.
    fn near_listener(&self, p: [f32; 2], r: f32) -> bool {
        self.racers.iter().any(|q| {
            let (dx, dy) = (q[0] - p[0], q[1] - p[1]);
            dx * dx + dy * dy <= r * r
        })
    }
}

/// A spoken line.
#[derive(Debug, Clone, PartialEq)]
pub struct SpokenLine {
    /// Speaking unit (`None` = dispatch, or several units collapsed into one line).
    pub unit: Option<UnitId>,
    /// Text.
    pub text: String,
    /// Tier it was raised with (the highest of collapsed lines).
    pub tier: RadioTier,
    /// Estimated transmission length, s (the channel is busy this long plus `min_gap_s`).
    pub duration_s: f32,
    /// Cut the transmission on air (a tier 1–2 line pre-empting a lower one, or emergency
    /// traffic): the only way two lines meet.
    pub cut_in: bool,
    /// The racer the line is about, if any.
    pub racer: Option<EntityId>,
}

#[derive(Debug, Clone, PartialEq)]
struct Member {
    unit: Option<UnitId>,
    callsign: Option<String>,
    text: String,
}

#[derive(Debug, Clone, PartialEq)]
struct Held {
    raised: SimTime,
    tier: RadioTier,
    prio: u8,
    ttl_s: Option<f32>,
    group: Option<RadioGroup>,
    slot: Option<String>,
    break_in: bool,
    racer: Option<EntityId>,
    at: Option<[f32; 2]>,
    members: Vec<Member>,
}

/// Holds raised lines and decides which are spoken. See the module docs.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RadioDirector {
    held: Vec<Held>,
    last_line: Option<SimTime>,
    /// The channel is busy (a transmission plus the silence after it) until then.
    busy_until: Option<SimTime>,
    /// The transmission itself ends then (tier 1 lines need no silence after it).
    tx_end: Option<SimTime>,
    /// The transmission on air is a break-in (only a higher tier cuts it).
    on_air_break_in: bool,
    /// Tier of the transmission on air.
    on_air_tier: Option<RadioTier>,
    /// Its `prio`.
    on_air_prio: u8,
    /// Last tier 1–3 line spoken (routine lines keep quiet for a while after it).
    last_important: Option<SimTime>,
    last_unit: BTreeMap<UnitId, SimTime>,
    /// Start times of the lines spoken in the last minute (line budget).
    recent: Vec<SimTime>,
    /// The one listener, if there is exactly one: lines name it "the suspect" instead of
    /// "racer #N" (the player never knows their entity number).
    suspect: Option<EntityId>,
    stats: RadioStats,
}

impl RadioDirector {
    /// Empty director.
    pub fn new() -> Self {
        Self::default()
    }

    /// Counters so far.
    pub fn stats(&self) -> RadioStats {
        self.stats
    }

    /// Lines waiting to be spoken.
    pub fn held(&self) -> usize {
        self.held.len()
    }

    /// Tier of the transmission on air at `now` (`None`: the channel is free).
    pub fn on_air(&self, now: SimTime) -> Option<RadioTier> {
        self.busy_until.filter(|t| now < *t).and(self.on_air_tier)
    }

    /// Withdraw the held (unspoken) line with this supersede slot. Returns whether there was one.
    pub fn withdraw(&mut self, slot: &str) -> bool {
        let before = self.held.len();
        self.held.retain(|h| h.slot.as_deref() != Some(slot));
        let n = before - self.held.len();
        self.stats.dropped += n as u32;
        n > 0
    }

    /// Raise a line at `now`. It merges into a held line with the same group key.
    pub fn raise(&mut self, now: SimTime, cue: RadioCue) {
        self.stats.raised += 1;
        if let Some(slot) = &cue.slot {
            let before = self.held.len();
            self.held.retain(|h| h.slot.as_ref() != Some(slot));
            self.stats.dropped += (before - self.held.len()) as u32;
        }
        let member = Member {
            unit: cue.unit,
            callsign: cue.callsign,
            text: cue.text,
        };
        if let Some(g) = &cue.group {
            if let Some(h) = self
                .held
                .iter_mut()
                .find(|h| h.group.as_ref().is_some_and(|hg| hg.key == g.key))
            {
                h.tier = h.tier.max(cue.tier);
                h.prio = h.prio.max(cue.prio);
                let dup = member.unit.is_some() && h.members.iter().any(|m| m.unit == member.unit);
                if dup {
                    // Same unit, same thing: one mention is enough.
                    self.stats.collapsed += 1;
                } else {
                    h.members.push(member);
                }
                return;
            }
        }
        self.held.push(Held {
            raised: now,
            tier: cue.tier,
            prio: cue.prio,
            ttl_s: cue.ttl_s,
            group: cue.group,
            slot: cue.slot,
            break_in: cue.break_in,
            racer: cue.racer,
            at: cue.at,
            members: vec![member],
        });
    }

    /// Decide what is spoken now: at most one line, and only when the channel is free or a
    /// waiting line may cut the one on air. Call once per police step, after all lines were
    /// raised.
    pub fn flush(&mut self, cfg: &RadioConfig, ctx: &RadioContext) -> Vec<SpokenLine> {
        let mut spoken = Vec::new();
        if self.held.is_empty() {
            return spoken;
        }
        self.suspect = match ctx.listeners {
            Some([one]) => Some(*one),
            _ => None,
        };
        let now = ctx.now;
        // 1. Drop stale lines (every tier, even while the channel is busy).
        let mut k = 0;
        while k < self.held.len() {
            let h = &self.held[k];
            let limit = h.ttl_s.unwrap_or(cfg.stale(h.tier));
            if now.since(h.raised) > limit {
                let h = self.held.remove(k);
                self.stats.dropped += h.members.len() as u32;
            } else {
                k += 1;
            }
        }
        // 1b. Lines that concern no listener (another racer's chase across the map, a unit
        // crashing far away) and, when switched off, routine chatter.
        let mut k = 0;
        while k < self.held.len() {
            let h = &self.held[k];
            let routine_off = h.tier == RadioTier::Routine && !cfg.routine;
            if routine_off || !Self::relevant(h, cfg, ctx) {
                let h = self.held.remove(k);
                self.stats.dropped += h.members.len() as u32;
                if !routine_off {
                    self.stats.irrelevant += h.members.len() as u32;
                }
            } else {
                k += 1;
            }
        }
        if self.held.is_empty() {
            return spoken;
        }
        // 2. Routine lines are dropped first when the channel is busy: anything more
        // important waiting, and they go.
        if self.held.iter().any(|h| h.tier > RadioTier::Routine) {
            let before: usize = self.held.iter().map(|h| h.members.len()).sum();
            self.held
                .retain(|h| h.tier > RadioTier::Routine || h.break_in);
            let after: usize = self.held.iter().map(|h| h.members.len()).sum();
            self.stats.dropped += (before - after) as u32;
        }
        // 3. One transmission at a time; a higher tier (or emergency traffic) cuts it. Tier 1
        // waits only for the transmission, not the silence after it.
        let t1_waiting = self.held.iter().any(|h| h.tier == RadioTier::Actionable);
        let busy = if t1_waiting {
            self.tx_end.is_some_and(|t| now < t)
        } else {
            self.busy_until.is_some_and(|t| now < t)
        };
        if busy {
            let on_air = self.on_air_tier.unwrap_or(RadioTier::Routine);
            let on_air_prio = self.on_air_prio;
            let on_air_break_in = self.on_air_break_in;
            // Inside tier 1, a tool warning (prio >= TOOL_WARNING_PRIO) cuts the other tier 1
            // traffic (helicopter, intercepts): it is the one that gets stale in seconds.
            let tool_over_t1 = |h: &Held| {
                h.tier == RadioTier::Actionable
                    && on_air == RadioTier::Actionable
                    && h.prio >= TOOL_WARNING_PRIO
                    && on_air_prio < TOOL_WARNING_PRIO
            };
            // Emergency traffic on air is only cut by tier 1; it cuts any non-break-in line
            // below tier 1 itself.
            let may_cut = |h: &Held| {
                if on_air_break_in {
                    h.tier.cuts(on_air) && h.tier == RadioTier::Actionable
                } else {
                    h.tier.cuts(on_air)
                        || tool_over_t1(h)
                        || (h.break_in && on_air < RadioTier::Actionable)
                }
            };
            let mut best: Option<usize> = None;
            for k in 0..self.held.len() {
                if !may_cut(&self.held[k]) {
                    continue;
                }
                if best.is_none_or(|b| Self::key(&self.held[k]) > Self::key(&self.held[b])) {
                    best = Some(k);
                }
            }
            let Some(b) = best else {
                return spoken;
            };
            let h = self.held.remove(b);
            let break_in = h.break_in;
            let mut line = self.speak(cfg, now, h);
            line.cut_in = true;
            self.stats.cuts += 1;
            self.busy_until = Some(now.plus(line.duration_s + cfg.after_line(line.tier)));
            self.tx_end = Some(now.plus(line.duration_s));
            self.on_air_break_in = break_in;
            spoken.push(line);
            return spoken;
        }
        // In the silence after a line only tier 1 may start.
        let silence = self.busy_until.is_some_and(|t| now < t);
        // Line budget: tiers 3-5 wait while the last minute already had enough lines.
        self.recent.retain(|t| now.since(*t) < 60.0);
        let budget_left = (self.recent.len() as f32) < cfg.max_per_min(ctx.state);
        let unit_gap = cfg.unit_gap(ctx.level);
        let quiet_after_important = self
            .last_important
            .is_none_or(|t| now.since(t) >= cfg.routine_quiet_s);
        let mut best: Option<usize> = None;
        let mut k = 0;
        while k < self.held.len() {
            let age = now.since(self.held[k].raised);
            let tier = self.held[k].tier;
            if self.held[k].group.is_some() && age < cfg.collapse_window_s {
                k += 1;
                continue;
            }
            let gap_open = self
                .last_line
                .is_none_or(|t| now.since(t) >= cfg.gap(tier, ctx.level))
                && (!silence || tier == RadioTier::Actionable);
            let over_budget = tier <= RadioTier::Impact && !budget_left;
            if !gap_open || over_budget || (tier == RadioTier::Routine && !quiet_after_important) {
                k += 1;
                continue;
            }
            if tier <= RadioTier::Investigation {
                let before = self.held[k].members.len();
                let last_unit = &self.last_unit;
                self.held[k].members.retain(|m| {
                    let audible = tier > RadioTier::Routine
                        || ctx.level > 0
                        || m.unit.and_then(|u| (ctx.unit_pos)(u)).is_some_and(|p| {
                            ctx.racers.iter().any(|r| {
                                let (dx, dy) = (r[0] - p[0], r[1] - p[1]);
                                dx * dx + dy * dy <= cfg.earshot_m * cfg.earshot_m
                            })
                        });
                    let rested = m
                        .unit
                        .and_then(|u| last_unit.get(&u))
                        .is_none_or(|t| now.since(*t) >= unit_gap);
                    audible && rested
                });
                self.stats.dropped += (before - self.held[k].members.len()) as u32;
                if self.held[k].members.is_empty() {
                    self.held.remove(k);
                    continue;
                }
            }
            if best.is_none_or(|b| Self::key(&self.held[k]) > Self::key(&self.held[b])) {
                best = Some(k);
            }
            k += 1;
        }
        if let Some(b) = best {
            let h = self.held.remove(b);
            self.on_air_break_in = h.break_in;
            let line = self.speak(cfg, now, h);
            self.busy_until = Some(now.plus(line.duration_s + cfg.after_line(line.tier)));
            self.tx_end = Some(now.plus(line.duration_s));
            spoken.push(line);
        }
        spoken
    }

    /// Queue order: tier 1 first, then emergency traffic (the officer who was just rammed
    /// shouts before dispatch calls the pursuit), then higher tier, then higher prio (ties: the
    /// earlier, i.e. older, line wins because only a strictly greater key replaces the best).
    fn key(h: &Held) -> (bool, bool, RadioTier, u8) {
        (h.tier == RadioTier::Actionable, h.break_in, h.tier, h.prio)
    }

    /// Whether a held line concerns a listener (always, without listeners): it is about a
    /// listener, or it happens (the racer, the place or the speaking unit) within
    /// `relevant_m` of one. Tier 3 lines need the place or the speaker within `impact_m`.
    fn relevant(h: &Held, cfg: &RadioConfig, ctx: &RadioContext) -> bool {
        let Some(listeners) = ctx.listeners else {
            return true;
        };
        let speaker = h
            .members
            .iter()
            .find_map(|m| m.unit.and_then(|u| (ctx.unit_pos)(u)));
        let place = h.at.or(speaker);
        if h.tier == RadioTier::Impact {
            if let Some(p) = place {
                return ctx.near_listener(p, cfg.impact_m);
            }
        }
        match h.racer {
            Some(r) if listeners.contains(&r) => true,
            Some(r) => (ctx.racer_pos)(r).is_some_and(|p| ctx.near_listener(p, cfg.relevant_m)),
            None => place.is_none_or(|p| ctx.near_listener(p, cfg.relevant_m)),
        }
    }

    fn speak(&mut self, cfg: &RadioConfig, now: SimTime, h: Held) -> SpokenLine {
        self.recent.push(now);
        self.stats.spoken += 1;
        self.stats.spoken_by_tier[h.tier.number() as usize - 1] += 1;
        self.stats.collapsed += h.members.len().saturating_sub(1) as u32;
        self.last_line = Some(now);
        self.on_air_tier = Some(h.tier);
        self.on_air_prio = h.prio;
        if h.tier.important() {
            self.last_important = Some(now);
        }
        for m in &h.members {
            if let Some(u) = m.unit {
                self.last_unit.insert(u, now);
            }
        }
        let (unit, text) = if h.members.len() == 1 {
            let m = h.members.into_iter().next().expect("one member");
            (m.unit, m.text)
        } else {
            let template = h.group.map(|g| g.text).unwrap_or_default();
            let signs: Vec<String> = h
                .members
                .iter()
                .filter_map(|m| m.callsign.clone())
                .collect();
            (
                None,
                template
                    .replace("{units}", &join_callsigns(&signs))
                    .replace("{count}", &h.members.len().to_string()),
            )
        };
        let text = match self.suspect {
            Some(r) => name_suspect(&text, r),
            None => text,
        };
        SpokenLine {
            unit,
            tier: h.tier,
            duration_s: cfg.line_s(&text),
            cut_in: false,
            racer: h.racer,
            text,
        }
    }
}

/// `text` with the racer reference "racer #N" for `racer` replaced by "the suspect" (the
/// single player's radio: they do not know their entity number). Other racers keep their
/// number. Whole references only ("racer #12" is not "racer #1").
pub fn name_suspect(text: &str, racer: EntityId) -> String {
    let token = format!("racer #{}", racer.0);
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find(&token) {
        let after = &rest[i + token.len()..];
        let whole = !after.starts_with(|c: char| c.is_ascii_digit());
        out.push_str(&rest[..i]);
        out.push_str(if whole { "the suspect" } else { &token });
        rest = after;
    }
    out.push_str(rest);
    out
}

/// The racer a line names ("racer #N"), if any: how `PoliceSystem::radio` attributes lines
/// built without [`RadioCue::about`].
pub fn racer_in_text(text: &str) -> Option<EntityId> {
    let i = text.find("racer #")?;
    let digits: String = text[i + 7..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.parse().ok().map(EntityId)
}

/// A tier 1 line with at least this `prio` is a roadblock / spike warning: it may cut other
/// tier 1 traffic on air (see [`RadioDirector::flush`]).
pub const TOOL_WARNING_PRIO: u8 = 2;

/// A grouped line lists at most this many call signs; more are said as a count.
pub const MAX_LISTED_CALLSIGNS: usize = 4;

/// "Unit 4", "Unit 5", "Unit 7" → "Units 4, 5 and 7"; mixed kinds keep full call signs
/// ("Unit 4 and Air 9"); more than [`MAX_LISTED_CALLSIGNS`]: "13 units".
pub fn join_callsigns(signs: &[String]) -> String {
    // More than a handful: say the count. "Units 1, 2, …, 13 resuming patrol" took ≈ 25 s of air
    // time and held critical traffic (the pursuit call, lost visual) back that long (main's
    // rebuilt grid, radio_chatter test).
    if signs.len() > MAX_LISTED_CALLSIGNS {
        return format!("{} units", signs.len());
    }
    let split: Vec<(&str, &str)> = signs
        .iter()
        .map(|s| s.split_once(' ').unwrap_or((s.as_str(), "")))
        .collect();
    let same_kind = signs.len() > 1 && split.iter().all(|(k, n)| *k == split[0].0 && !n.is_empty());
    let parts: Vec<&str> = if same_kind {
        split.iter().map(|(_, n)| *n).collect()
    } else {
        signs.iter().map(|s| s.as_str()).collect()
    };
    let list = match parts.as_slice() {
        [] => String::new(),
        [one] => one.to_string(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    };
    if same_kind {
        format!("{}s {list}", split[0].0)
    } else {
        list
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> RadioConfig {
        RadioConfig {
            global_gap_s: [8.0, 6.5, 5.0, 4.0, 3.5],
            global_gap_calm_s: 20.0,
            unit_gap_s: [30.0, 22.0, 16.0, 12.0, 10.0],
            collapse_window_s: 1.0,
            stale_s: 6.0,
            stale_critical_s: 15.0,
            routine_gap_s: [10.0, 14.0, 20.0, 30.0, 45.0],
            routine_quiet_s: 6.0,
            impact_gap_s: 0.4,
            impact_stale_s: 4.0,
            impact_min_gap_s: 0.8,
            earshot_m: 200.0,
            min_gap_s: 2.5,
            words_per_s: 2.6,
            line_pad_s: 0.7,
            impact: ImpactConfig::default(),
            takeover: d_takeover(),
            relevant_m: d_relevant(),
            impact_m: d_impact_m(),
            // The pacing tests below are about gaps and cuts, not the budget.
            max_lines_per_min: [1000.0; 3],
            routine: true,
        }
    }

    fn no_racer(_: EntityId) -> Option<[f32; 2]> {
        None
    }

    fn patrol(u: u32) -> RadioCue {
        RadioCue::new(
            RadioTier::Routine,
            Some(UnitId(u)),
            format!("Unit {u} resuming patrol, Core."),
        )
        .grouped(
            Some(format!("Unit {u}")),
            "patrol".into(),
            "{units} resuming patrol.".into(),
        )
    }

    fn near(_: UnitId) -> Option<[f32; 2]> {
        Some([0.0, 0.0])
    }
    fn far(_: UnitId) -> Option<[f32; 2]> {
        Some([5000.0, 0.0])
    }

    /// Flush every 0.1 s from `from` to `to`; spoken lines with their start time.
    fn run(
        d: &mut RadioDirector,
        from: f32,
        to: f32,
        level: u8,
        pos: fn(UnitId) -> Option<[f32; 2]>,
    ) -> Vec<(f32, SpokenLine)> {
        let mut out = Vec::new();
        let mut t = from;
        while t < to {
            let ctx = RadioContext {
                now: SimTime::from_secs(t),
                level,
                state: ListenerState::Calm,
                racers: &[[0.0, 0.0]],
                listeners: None,
                racer_pos: &no_racer,
                unit_pos: &pos,
            };
            for l in d.flush(&cfg(), &ctx) {
                out.push((t, l));
            }
            t += 0.1;
        }
        out
    }

    /// Every transmission ends (plus the minimum silence) before the next one starts, unless
    /// the next one is emergency traffic cutting it.
    fn assert_no_overlap(lines: &[(f32, SpokenLine)]) {
        for w in lines.windows(2).filter(|w| !w[1].1.cut_in) {
            let silence = if w[1].1.tier == RadioTier::Actionable {
                0.0
            } else {
                cfg().after_line(w[0].1.tier)
            };
            let end = w[0].0 + w[0].1.duration_s + silence;
            assert!(
                w[1].0 + 1e-3 >= end,
                "'{}' at {:.1} s starts before '{}' (from {:.1} s) is over at {:.1} s",
                w[1].1.text,
                w[1].0,
                w[0].1.text,
                w[0].0,
                end
            );
        }
    }

    #[test]
    fn duplicates_collapse_into_one_line() {
        let mut d = RadioDirector::new();
        for u in [4, 5, 7] {
            d.raise(SimTime::ZERO, patrol(u));
        }
        let out = run(&mut d, 0.0, 3.0, 1, near);
        assert_eq!(out.len(), 1, "{out:?}");
        assert_eq!(out[0].1.text, "Units 4, 5 and 7 resuming patrol.");
        assert_eq!(d.stats().collapsed, 2);
    }

    #[test]
    fn lone_line_keeps_its_own_text() {
        let mut d = RadioDirector::new();
        d.raise(SimTime::ZERO, patrol(4));
        let out = run(&mut d, 0.0, 3.0, 1, near);
        assert_eq!(out[0].1.text, "Unit 4 resuming patrol, Core.");
        assert_eq!(out[0].1.unit, Some(UnitId(4)));
        assert!((out[0].1.duration_s - (5.0 / 2.6 + 0.7)).abs() < 1e-4);
    }

    #[test]
    fn patrol_chatter_is_off_out_of_earshot_when_nobody_is_wanted() {
        let mut d = RadioDirector::new();
        d.raise(SimTime::ZERO, patrol(4));
        assert!(run(&mut d, 0.0, 10.0, 0, far).is_empty());
        assert_eq!(d.stats().dropped, 1);
        d.raise(SimTime::from_secs(10.0), patrol(5));
        assert_eq!(run(&mut d, 10.0, 20.0, 0, near).len(), 1);
    }

    #[test]
    fn critical_lines_queue_one_at_a_time_and_routine_waits_for_the_gap() {
        let mut d = RadioDirector::new();
        let crit = |t: &str| RadioCue::new(RadioTier::Escalation, None, t.into());
        d.raise(SimTime::ZERO, crit("Dispatch: pursuit."));
        d.raise(SimTime::ZERO, patrol(4));
        d.raise(SimTime::ZERO, crit("Dispatch: roadblock set."));
        let out = run(&mut d, 0.0, 20.0, 1, near);
        let texts: Vec<&str> = out.iter().map(|l| l.1.text.as_str()).collect();
        // Both critical lines, in the order raised, never on top of each other. The routine
        // line waits for the 8 s gap and is stale (6 s) before it opens.
        assert_eq!(texts, ["Dispatch: pursuit.", "Dispatch: roadblock set."]);
        assert_eq!(out[0].0, 0.0);
        assert_no_overlap(&out);
    }

    #[test]
    fn priority_orders_the_queue() {
        let mut d = RadioDirector::new();
        let t = |p: u8, s: &str| RadioCue::new(RadioTier::Investigation, None, s.into()).prio(p);
        d.raise(SimTime::ZERO, t(0, "Dispatch: copy, sending backup."));
        d.raise(
            SimTime::ZERO,
            RadioCue::new(
                RadioTier::Investigation,
                Some(UnitId(4)),
                "Unit 4, need backup.".into(),
            )
            .prio(2),
        );
        d.raise(
            SimTime::ZERO,
            RadioCue::new(RadioTier::Routine, None, "x".into()).prio(9),
        );
        let mut c = cfg();
        c.stale_s = 30.0;
        let mut out = Vec::new();
        let mut now = 0.0;
        while now < 30.0 {
            let ctx = RadioContext {
                now: SimTime::from_secs(now),
                level: 3,
                state: ListenerState::Calm,
                racers: &[[0.0, 0.0]],
                listeners: None,
                racer_pos: &no_racer,
                unit_pos: &near,
            };
            out.extend(d.flush(&c, &ctx).into_iter().map(|l| l.text));
            now += 0.1;
        }
        // The routine line is dropped at once: more important lines were waiting (row 144).
        assert_eq!(
            out,
            ["Unit 4, need backup.", "Dispatch: copy, sending backup."]
        );
    }

    #[test]
    fn a_newer_line_supersedes_a_held_one() {
        let mut d = RadioDirector::new();
        let lvl = |n: u8| {
            RadioCue::new(
                RadioTier::Escalation,
                None,
                format!("Dispatch: now level {n}."),
            )
            .supersedes("level:1".into())
        };
        d.raise(
            SimTime::ZERO,
            RadioCue::new(RadioTier::Escalation, None, "Dispatch: 10-80.".into()),
        );
        d.raise(SimTime::ZERO, lvl(2));
        d.raise(SimTime::from_secs(1.0), lvl(3));
        let out: Vec<String> = run(&mut d, 0.0, 15.0, 3, near)
            .into_iter()
            .map(|l| l.1.text)
            .collect();
        assert_eq!(out, ["Dispatch: 10-80.", "Dispatch: now level 3."]);
    }

    #[test]
    fn emergency_traffic_cuts_in_once() {
        let mut d = RadioDirector::new();
        let long =
            "Dispatch: a long routine line that keeps the channel busy for quite a while now.";
        d.raise(
            SimTime::ZERO,
            RadioCue::new(RadioTier::Escalation, None, long.into()),
        );
        let mut out = run(&mut d, 0.0, 0.5, 3, near);
        let hit = |n: u32| {
            RadioCue::new(
                RadioTier::Escalation,
                Some(UnitId(n)),
                format!("Unit {n}, I've been hit!"),
            )
            .breaks_in()
        };
        d.raise(SimTime::from_secs(0.5), hit(4));
        d.raise(SimTime::from_secs(0.5), hit(5));
        out.extend(run(&mut d, 0.5, 20.0, 3, near));
        let texts: Vec<&str> = out.iter().map(|l| l.1.text.as_str()).collect();
        assert_eq!(
            texts,
            [long, "Unit 4, I've been hit!", "Unit 5, I've been hit!"]
        );
        // Unit 4 cut the dispatcher at once; unit 5 waited for unit 4 (no cutting a break-in).
        assert!(out[1].1.cut_in && (out[1].0 - 0.5).abs() < 0.11);
        assert!(!out[2].1.cut_in);
        assert_no_overlap(&out);
    }

    #[test]
    fn short_lived_callouts_expire() {
        let mut d = RadioDirector::new();
        d.raise(
            SimTime::ZERO,
            RadioCue::new(
                RadioTier::Escalation,
                None,
                "Dispatch: a long line to keep the channel busy for a while.".into(),
            ),
        );
        d.raise(
            SimTime::ZERO,
            RadioCue::new(
                RadioTier::Investigation,
                Some(UnitId(3)),
                "Unit 3, on his bumper.".into(),
            )
            .ttl(2.0),
        );
        let out = run(&mut d, 0.0, 15.0, 5, near);
        assert_eq!(out.len(), 1, "{out:?}");
        assert_eq!(d.stats().dropped, 1);
    }

    /// A unit status line every second from a rotating set of 10 units, at wanted level
    /// `level`, for 60 s: spoken start times.
    fn flood(level: u8) -> Vec<(f32, SpokenLine)> {
        let mut d = RadioDirector::new();
        let mut out = Vec::new();
        let mut t = 0.0f32;
        while t < 60.0 {
            if (t * 10.0).round() as i32 % 10 == 0 {
                let u = (t as u32) % 10 + 1;
                d.raise(
                    SimTime::from_secs(t),
                    RadioCue::new(
                        RadioTier::Investigation,
                        Some(UnitId(u)),
                        format!("Unit {u} en route."),
                    ),
                );
            }
            let ctx = RadioContext {
                now: SimTime::from_secs(t),
                level,
                state: ListenerState::Calm,
                racers: &[[0.0, 0.0]],
                listeners: None,
                racer_pos: &no_racer,
                unit_pos: &near,
            };
            for l in d.flush(&cfg(), &ctx) {
                out.push((t, l));
            }
            t += 0.1;
        }
        out
    }

    #[test]
    fn level_one_keeps_the_row_6_spacing() {
        let lines = flood(1);
        assert!(lines.len() <= 8, "{lines:?}");
        assert!(
            lines.windows(2).all(|w| w[1].0 - w[0].0 >= 7.99),
            "{lines:?}"
        );
        assert_no_overlap(&lines);
    }

    #[test]
    fn more_chatter_as_the_wanted_level_rises_but_never_overlapping() {
        let counts: Vec<usize> = (1..=5).map(|l| flood(l).len()).collect();
        assert!(counts.windows(2).all(|w| w[1] >= w[0]), "{counts:?}");
        assert!(counts[4] > counts[0], "{counts:?}");
        for l in 1..=5 {
            assert_no_overlap(&flood(l));
        }
    }

    fn cue(tier: RadioTier, text: &str) -> RadioCue {
        RadioCue::new(tier, None, text.into())
    }

    const LONG: &str = "Dispatch: a long line that keeps the channel busy for quite a while, more \
                        than long enough to be cut by something that matters.";

    #[test]
    fn tier_one_cuts_a_routine_line_mid_air() {
        let mut d = RadioDirector::new();
        d.raise(SimTime::ZERO, cue(RadioTier::Routine, LONG));
        let mut out = run(&mut d, 0.0, 1.0, 1, near);
        assert_eq!(out.len(), 1, "{out:?}");
        d.raise(
            SimTime::from_secs(1.0),
            cue(
                RadioTier::Actionable,
                "Dispatch: roadblock ahead on Lantern Row, 400 metres.",
            ),
        );
        out.extend(run(&mut d, 1.0, 3.0, 1, near));
        assert_eq!(out.len(), 2, "{out:?}");
        assert!(out[1].1.cut_in, "{out:?}");
        assert!((out[1].0 - 1.0).abs() < 0.11, "spoken at {}", out[1].0);
        assert_eq!(d.stats().cuts, 1);
    }

    #[test]
    fn tier_one_cuts_every_lower_tier_and_tier_two_cuts_three_to_five() {
        for on_air in [
            RadioTier::Routine,
            RadioTier::Investigation,
            RadioTier::Impact,
            RadioTier::Escalation,
            RadioTier::Actionable,
        ] {
            for waiting in [RadioTier::Escalation, RadioTier::Actionable] {
                let mut d = RadioDirector::new();
                d.raise(SimTime::ZERO, cue(on_air, LONG));
                let mut out = run(&mut d, 0.0, 0.5, 3, near);
                d.raise(SimTime::from_secs(0.5), cue(waiting, "Dispatch: now."));
                out.extend(run(&mut d, 0.5, 1.0, 3, near));
                let cut = out.len() == 2;
                assert_eq!(
                    cut,
                    waiting > on_air,
                    "{waiting:?} over {on_air:?}: {out:?}"
                );
            }
        }
    }

    #[test]
    fn routine_lines_are_dropped_when_busy() {
        let mut d = RadioDirector::new();
        d.raise(SimTime::ZERO, cue(RadioTier::Escalation, LONG));
        d.raise(SimTime::ZERO, cue(RadioTier::Escalation, LONG));
        d.raise(SimTime::ZERO, patrol(4));
        let out = run(&mut d, 0.0, 60.0, 1, near);
        assert!(
            out.iter().all(|l| l.1.tier != RadioTier::Routine),
            "{out:?}"
        );
        assert_eq!(d.stats().dropped, 1);
        // On a quiet channel the same line is spoken.
        let mut d = RadioDirector::new();
        d.raise(SimTime::ZERO, patrol(4));
        assert_eq!(run(&mut d, 0.0, 5.0, 1, near).len(), 1);
    }

    #[test]
    fn routine_keeps_quiet_after_an_important_line() {
        let mut d = RadioDirector::new();
        d.raise(
            SimTime::ZERO,
            cue(RadioTier::Impact, "Unit 4, I'm hit! I'm hit!"),
        );
        let mut out = run(&mut d, 0.0, 3.0, 0, near);
        d.raise(SimTime::from_secs(3.0), patrol(5));
        out.extend(run(&mut d, 3.0, 30.0, 0, near));
        // Calm gap (20 s) after the impact line: the routine line is stale (6 s) before then.
        assert_eq!(out.len(), 1, "{out:?}");
    }

    #[test]
    fn a_big_wreck_stacks_two_quick_impact_lines() {
        let mut d = RadioDirector::new();
        d.raise(
            SimTime::ZERO,
            RadioCue::new(
                RadioTier::Impact,
                Some(UnitId(4)),
                "Unit 4, I'm done! I'm done!".into(),
            )
            .prio(2),
        );
        d.raise(
            SimTime::ZERO,
            RadioCue::new(
                RadioTier::Impact,
                Some(UnitId(6)),
                "Unit 6, Unit 4 is down, Unit 4 is down!".into(),
            )
            .prio(1)
            .ttl(6.0),
        );
        let out = run(&mut d, 0.0, 10.0, 3, near);
        assert_eq!(out.len(), 2, "{out:?}");
        assert!(out[1].1.text.contains("is down"));
        let end = out[0].0 + out[0].1.duration_s + cfg().impact_min_gap_s;
        assert!(out[1].0 - end < cfg().impact_gap_s + 0.15, "{out:?}");
        assert_no_overlap(&out);
    }

    #[test]
    fn routine_thins_out_as_heat_rises() {
        let c = cfg();
        let gaps: Vec<f32> = (1..=5).map(|l| c.routine_gap(l)).collect();
        assert!(gaps.windows(2).all(|w| w[1] >= w[0]), "{gaps:?}");
        assert_eq!(c.gap(RadioTier::Actionable, 5), 0.0);
        assert!(c.gap(RadioTier::Impact, 3) < c.gap(RadioTier::Investigation, 3));
    }

    #[test]
    fn old_logs_still_parse() {
        #[derive(Deserialize)]
        struct L {
            tier: RadioTier,
        }
        let crit: L = serde_json::from_str(r#"{"tier":"Critical"}"#).unwrap();
        let tac: L = serde_json::from_str(r#"{"tier":"Tactical"}"#).unwrap();
        assert_eq!(crit.tier, RadioTier::Escalation);
        assert_eq!(tac.tier, RadioTier::Investigation);
        assert_eq!(RadioTier::Actionable.number(), 1);
        assert!(RadioTier::Impact.important() && !RadioTier::Investigation.important());
    }

    #[test]
    fn callsign_lists() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(join_callsigns(&s(&["Unit 4", "Unit 5"])), "Units 4 and 5");
        assert_eq!(join_callsigns(&s(&["Unit 4", "Air 9"])), "Unit 4 and Air 9");
        assert_eq!(join_callsigns(&s(&["Unit 4"])), "Unit 4");
    }

    /// One listener (racer #1 at the origin); racer #2 is `npc_m` metres away; units sit at
    /// x = their id × 100 m. Spoken texts over `secs`.
    fn listen(
        d: &mut RadioDirector,
        c: &RadioConfig,
        state: ListenerState,
        npc_m: f32,
        secs: f32,
    ) -> Vec<(f32, String)> {
        let listeners = [EntityId(1)];
        let racer_pos = move |r: EntityId| match r.0 {
            1 => Some([0.0, 0.0]),
            2 => Some([npc_m, 0.0]),
            _ => None,
        };
        let unit_pos = |u: UnitId| Some([u.0 as f32 * 100.0, 0.0]);
        let mut out = Vec::new();
        let mut t = 0.0;
        while t < secs {
            let ctx = RadioContext {
                now: SimTime::from_secs(t),
                level: 2,
                state,
                racers: &[[0.0, 0.0]],
                listeners: Some(&listeners),
                racer_pos: &racer_pos,
                unit_pos: &unit_pos,
            };
            out.extend(d.flush(c, &ctx).into_iter().map(|l| (t, l.text)));
            t += 0.1;
        }
        out
    }

    fn texts(v: Vec<(f32, String)>) -> Vec<String> {
        v.into_iter().map(|x| x.1).collect()
    }

    #[test]
    fn another_racers_chase_far_away_is_not_heard() {
        let c = cfg();
        let mut d = RadioDirector::new();
        let t0 = SimTime::ZERO;
        d.raise(
            t0,
            RadioCue::new(
                RadioTier::Actionable,
                None,
                "Dispatch: roadblock ahead of racer #2.".into(),
            )
            .about(EntityId(2)),
        );
        d.raise(
            t0,
            RadioCue::new(
                RadioTier::Escalation,
                None,
                "Dispatch: pursuit of racer #1.".into(),
            )
            .about(EntityId(1)),
        );
        // A unit 2 km away crashing (no racer) and one 100 m away.
        d.raise(
            t0,
            RadioCue::new(
                RadioTier::Impact,
                Some(UnitId(20)),
                "Unit 20, into the wall!".into(),
            ),
        );
        d.raise(
            t0,
            RadioCue::new(
                RadioTier::Impact,
                Some(UnitId(1)),
                "Unit 1, into the wall!".into(),
            )
            .ttl(10.0),
        );
        let out = texts(listen(&mut d, &c, ListenerState::Pursuit, 1500.0, 20.0));
        assert_eq!(
            out,
            [
                "Dispatch: pursuit of the suspect.",
                "Unit 1, into the wall!"
            ],
            "only the listener's own business and what happens next to them"
        );
        assert_eq!(d.stats().irrelevant, 2);
        // The same chase right next to the listener is heard (with its racer number).
        let mut d = RadioDirector::new();
        d.raise(
            t0,
            RadioCue::new(
                RadioTier::Actionable,
                None,
                "Dispatch: roadblock ahead of racer #2.".into(),
            )
            .about(EntityId(2)),
        );
        let out = texts(listen(&mut d, &c, ListenerState::Calm, 150.0, 5.0));
        assert_eq!(out, ["Dispatch: roadblock ahead of racer #2."]);
    }

    #[test]
    fn the_line_budget_holds_back_tiers_3_to_5_only() {
        let mut c = cfg();
        c.max_lines_per_min = [2.0, 2.0, 2.0];
        c.global_gap_s = [0.5; 5];
        c.unit_gap_s = [0.5; 5];
        let mut d = RadioDirector::new();
        for k in 0..4u32 {
            d.raise(
                SimTime::ZERO,
                RadioCue::new(
                    RadioTier::Investigation,
                    Some(UnitId(1)),
                    format!("Unit 1, line {k}."),
                )
                .about(EntityId(1))
                .ttl(120.0),
            );
        }
        d.raise(
            SimTime::ZERO,
            RadioCue::new(
                RadioTier::Escalation,
                None,
                "Dispatch: pursuit of racer #1.".into(),
            )
            .about(EntityId(1)),
        );
        let out = listen(&mut d, &c, ListenerState::Pursuit, 1500.0, 130.0);
        // The tier 2 line and one tier 4 line fill the budget of 2; the rest wait for the
        // next minute.
        let first = out.iter().filter(|(t, _)| *t < 59.0).count();
        assert_eq!(first, 2, "{out:?}");
        assert!(out.len() >= 4, "the next minute has room again: {out:?}");
    }

    #[test]
    fn routine_lines_can_be_switched_off() {
        let mut c = cfg();
        c.routine = false;
        let mut d = RadioDirector::new();
        d.raise(SimTime::ZERO, patrol(1));
        let out = texts(listen(&mut d, &c, ListenerState::Calm, 1500.0, 40.0));
        assert!(out.is_empty(), "{out:?}");
    }

    #[test]
    fn the_single_listener_is_the_suspect() {
        assert_eq!(
            name_suspect(
                "Dispatch: racer #1 is now level 2; racer #12 too.",
                EntityId(1)
            ),
            "Dispatch: the suspect is now level 2; racer #12 too."
        );
        assert_eq!(
            racer_in_text("pursuit of racer #1002 eastbound"),
            Some(EntityId(1002))
        );
        assert_eq!(racer_in_text("Unit 4, boxing him in."), None);
    }
}
