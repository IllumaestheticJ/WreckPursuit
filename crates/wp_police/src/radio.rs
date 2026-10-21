//! Radio pacing: decides which police radio lines are spoken, and when.
//!
//! Every dispatch decision, state change and unit callout ([`crate::chatter`]) *raises* a
//! line ([`RadioDirector::raise`]); the director holds it and, once per police step
//! ([`RadioDirector::flush`]), decides what is actually transmitted (radio event + scanner
//! log). Rules, tuned in `data/police/radio.toml`:
//!
//! - **One transmission at a time** (playtest row 79). A spoken line keeps the channel busy for
//!   its estimated length (`words / words_per_s + line_pad_s`) plus `min_gap_s` of silence;
//!   nothing else is spoken until then, not even a critical line. Waiting lines form a priority
//!   queue: tier first, then the cue's `prio`, then the older line.
//! - **Tiers** ([`RadioTier`]). `Critical` lines (pursuit start, wanted level up, roadblock or
//!   spikes set, lost visual, suspect lost, capture, unit down, captain orders) skip the gaps
//!   below and live `stale_critical_s`. `Tactical` and `Routine` lines wait for the gaps and are
//!   dropped when they get stale (`stale_s`, or the cue's own `ttl_s`).
//! - **Global gap by wanted level.** After any spoken line, no tactical/routine line for
//!   `global_gap_s[level - 1]` (highest wanted level of any racer: more chatter as heat rises)
//!   or `global_gap_calm_s` (nobody wanted). L1 keeps the row-6 value (8 s).
//! - **Per-unit gap by wanted level.** A unit says at most one tactical/routine line per
//!   `unit_gap_s[level - 1]` (L1 / calm: 30 s, the row-6 value).
//! - **Collapse.** Lines raised with the same group key within `collapse_window_s` become one
//!   line ("Units 4 and 5 resuming patrol.").
//! - **Earshot.** Routine chatter needs a wanted racer, or the speaking unit within
//!   `earshot_m` of a racer (dispatch's own routine lines need a wanted racer).
//!
//! Deterministic: sim time only, no RNG, fixed iteration order. Nothing here feeds back into
//! the simulation or the state hash.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::events::UnitId;
use crate::time::SimTime;

/// How much a radio line matters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum RadioTier {
    /// Ambience: resuming patrol, clear and available, unrelated incidents.
    Routine,
    /// Tactical updates: units en route, intercepts, backup, helicopter, unit callouts.
    Tactical,
    /// Skips the gaps: pursuit start, wanted level up, tool set, lost visual, suspect lost,
    /// capture, unit down.
    Critical,
}

/// Serde default for logs written before tiers were recorded.
pub fn default_tier() -> RadioTier {
    RadioTier::Tactical
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
        }
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

/// Contents of `data/police/radio.toml`. All times are sim seconds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RadioConfig {
    /// Gap after any spoken line before a tactical/routine line, by the highest wanted level
    /// of any racer (L1..L5), s. L1 is the playtest row-6 value.
    pub global_gap_s: [f32; 5],
    /// Same while no racer is wanted, s.
    pub global_gap_calm_s: f32,
    /// Gap between one unit's tactical/routine lines, by wanted level (L1..L5; nobody wanted
    /// uses L1), s.
    pub unit_gap_s: [f32; 5],
    /// Tactical/routine lines wait this long for duplicates from other units to merge, s.
    pub collapse_window_s: f32,
    /// Held tactical/routine lines older than this are dropped, s.
    pub stale_s: f32,
    /// Held critical lines older than this are dropped, s.
    #[serde(default = "d_stale_critical")]
    pub stale_critical_s: f32,
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
}

impl RadioConfig {
    /// Estimated length of a transmission of `text`, s.
    pub fn line_s(&self, text: &str) -> f32 {
        let words = text.split_whitespace().count().max(1) as f32;
        words / self.words_per_s.max(0.5) + self.line_pad_s
    }

    /// Tactical/routine gap after any line at the highest wanted `level` (0 = nobody), s.
    pub fn global_gap(&self, level: u8) -> f32 {
        match level {
            0 => self.global_gap_calm_s,
            l => self.global_gap_s[(l as usize - 1).min(4)],
        }
    }

    /// Per-unit gap at the highest wanted `level` (0 = nobody, uses L1), s.
    pub fn unit_gap(&self, level: u8) -> f32 {
        self.unit_gap_s[(level.max(1) as usize - 1).min(4)]
    }
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
}

/// What the director needs to know about the moment of a flush.
pub struct RadioContext<'a> {
    /// Current sim time.
    pub now: SimTime,
    /// Highest wanted level of any racer (0 = nobody wanted, 1..=5).
    pub level: u8,
    /// Racer ground positions, m.
    pub racers: &'a [[f32; 2]],
    /// Ground position of a unit, m (`None` if gone).
    pub unit_pos: &'a dyn Fn(UnitId) -> Option<[f32; 2]>,
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
    /// Emergency traffic that cut the transmission on air (the only way two lines meet).
    pub cut_in: bool,
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
    members: Vec<Member>,
}

/// Holds raised lines and decides which are spoken. See the module docs.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RadioDirector {
    held: Vec<Held>,
    last_line: Option<SimTime>,
    /// The channel is busy (a transmission plus the silence after it) until then.
    busy_until: Option<SimTime>,
    /// The transmission on air is a break-in (cannot be cut).
    on_air_break_in: bool,
    last_unit: BTreeMap<UnitId, SimTime>,
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
            members: vec![member],
        });
    }

    /// Decide what is spoken now: at most one line, and only when the channel is free. Call
    /// once per police step, after all lines were raised.
    pub fn flush(&mut self, cfg: &RadioConfig, ctx: &RadioContext) -> Vec<SpokenLine> {
        let mut spoken = Vec::new();
        if self.held.is_empty() {
            return spoken;
        }
        let now = ctx.now;
        // 1. Drop stale lines (every tier, even while the channel is busy).
        let mut k = 0;
        while k < self.held.len() {
            let h = &self.held[k];
            let limit = h.ttl_s.unwrap_or(if h.tier == RadioTier::Critical {
                cfg.stale_critical_s
            } else {
                cfg.stale_s
            });
            if now.since(h.raised) > limit {
                let h = self.held.remove(k);
                self.stats.dropped += h.members.len() as u32;
            } else {
                k += 1;
            }
        }
        // 2. One transmission at a time; emergency traffic cuts a normal transmission.
        let busy = self.busy_until.is_some_and(|t| now < t);
        if busy {
            if self.on_air_break_in {
                return spoken;
            }
            let Some(b) = (0..self.held.len()).find(|&k| self.held[k].break_in) else {
                return spoken;
            };
            let h = self.held.remove(b);
            let mut line = self.speak(cfg, now, h);
            line.cut_in = true;
            self.busy_until = Some(now.plus(line.duration_s + cfg.min_gap_s));
            self.on_air_break_in = true;
            spoken.push(line);
            return spoken;
        }
        let gap_open = self
            .last_line
            .is_none_or(|t| now.since(t) >= cfg.global_gap(ctx.level));
        let unit_gap = cfg.unit_gap(ctx.level);
        let mut best: Option<usize> = None;
        let mut k = 0;
        while k < self.held.len() {
            let age = now.since(self.held[k].raised);
            let tier = self.held[k].tier;
            if self.held[k].group.is_some() && age < cfg.collapse_window_s {
                k += 1;
                continue;
            }
            if tier < RadioTier::Critical {
                if !gap_open {
                    k += 1;
                    continue;
                }
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
            // Break-ins first, then higher tier, higher prio, then the older line.
            let key = |h: &Held| (h.break_in, h.tier, h.prio);
            if best.is_none_or(|b| key(&self.held[k]) > key(&self.held[b])) {
                best = Some(k);
            }
            k += 1;
        }
        if let Some(b) = best {
            let h = self.held.remove(b);
            self.on_air_break_in = h.break_in;
            let line = self.speak(cfg, now, h);
            self.busy_until = Some(now.plus(line.duration_s + cfg.min_gap_s));
            spoken.push(line);
        }
        spoken
    }

    fn speak(&mut self, cfg: &RadioConfig, now: SimTime, h: Held) -> SpokenLine {
        self.stats.spoken += 1;
        self.stats.collapsed += h.members.len().saturating_sub(1) as u32;
        self.last_line = Some(now);
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
        SpokenLine {
            unit,
            tier: h.tier,
            duration_s: cfg.line_s(&text),
            cut_in: false,
            text,
        }
    }
}

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
            earshot_m: 200.0,
            min_gap_s: 2.5,
            words_per_s: 2.6,
            line_pad_s: 0.7,
        }
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
                racers: &[[0.0, 0.0]],
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
            let end = w[0].0 + w[0].1.duration_s + cfg().min_gap_s;
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
        let crit = |t: &str| RadioCue::new(RadioTier::Critical, None, t.into());
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
        let t = |p: u8, s: &str| RadioCue::new(RadioTier::Tactical, None, s.into()).prio(p);
        d.raise(SimTime::ZERO, t(0, "Dispatch: copy, sending backup."));
        d.raise(
            SimTime::ZERO,
            RadioCue::new(
                RadioTier::Tactical,
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
                racers: &[[0.0, 0.0]],
                unit_pos: &near,
            };
            out.extend(d.flush(&c, &ctx).into_iter().map(|l| l.text));
            now += 0.1;
        }
        assert_eq!(
            out,
            [
                "Unit 4, need backup.",
                "Dispatch: copy, sending backup.",
                "x"
            ]
        );
    }

    #[test]
    fn a_newer_line_supersedes_a_held_one() {
        let mut d = RadioDirector::new();
        let lvl = |n: u8| {
            RadioCue::new(
                RadioTier::Critical,
                None,
                format!("Dispatch: now level {n}."),
            )
            .supersedes("level:1".into())
        };
        d.raise(
            SimTime::ZERO,
            RadioCue::new(RadioTier::Critical, None, "Dispatch: 10-80.".into()),
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
            RadioCue::new(RadioTier::Critical, None, long.into()),
        );
        let mut out = run(&mut d, 0.0, 0.5, 3, near);
        let hit = |n: u32| {
            RadioCue::new(
                RadioTier::Critical,
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
                RadioTier::Critical,
                None,
                "Dispatch: a long line to keep the channel busy for a while.".into(),
            ),
        );
        d.raise(
            SimTime::ZERO,
            RadioCue::new(
                RadioTier::Tactical,
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
                        RadioTier::Tactical,
                        Some(UnitId(u)),
                        format!("Unit {u} en route."),
                    ),
                );
            }
            let ctx = RadioContext {
                now: SimTime::from_secs(t),
                level,
                racers: &[[0.0, 0.0]],
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

    #[test]
    fn callsign_lists() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(join_callsigns(&s(&["Unit 4", "Unit 5"])), "Units 4 and 5");
        assert_eq!(join_callsigns(&s(&["Unit 4", "Air 9"])), "Unit 4 and Air 9");
        assert_eq!(join_callsigns(&s(&["Unit 4"])), "Unit 4");
    }
}
