//! Radio pacing: decides which police radio lines are spoken.
//!
//! Every dispatch decision and state change *raises* a line ([`RadioDirector::raise`]); the
//! director holds it and, once per police step ([`RadioDirector::flush`]), decides what is
//! actually spoken (radio event + scanner log). Rules, tuned in `data/police/radio.toml`:
//!
//! - **Tiers** ([`RadioTier`]). `Critical` lines (pursuit start, wanted level up, roadblock or
//!   spikes set, suspect lost, capture, captain orders) are always spoken, in the step they
//!   were raised. `Tactical` and `Routine` lines wait for the cooldowns and are dropped when
//!   they get stale; when several are ready, the higher tier and then the older line wins.
//! - **Global cooldown.** After any spoken line, no tactical/routine line for
//!   `global_gap_s` (someone wanted) or `global_gap_calm_s` (nobody wanted).
//! - **Per-unit cooldown.** A unit says at most one tactical/routine line per `unit_gap_s`.
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
    /// Tactical updates: units en route, intercepts, backup, helicopter.
    Tactical,
    /// Always spoken: pursuit start, wanted level up, tool set, suspect lost, capture.
    Critical,
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

/// A line raised by dispatch logic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RadioCue {
    /// Priority tier.
    pub tier: RadioTier,
    /// Speaking unit (`None` = dispatch / captain).
    pub unit: Option<UnitId>,
    /// Unit call sign for collapsed lines ("Unit 4").
    pub callsign: Option<String>,
    /// Text when spoken alone.
    pub text: String,
    /// Collapse rule.
    pub group: Option<RadioGroup>,
}

impl RadioCue {
    /// A line with no collapse rule.
    pub fn new(tier: RadioTier, unit: Option<UnitId>, text: String) -> Self {
        Self {
            tier,
            unit,
            callsign: None,
            text,
            group: None,
        }
    }

    /// Add a collapse rule (`callsign` names this unit in the collapsed text).
    pub fn grouped(mut self, callsign: Option<String>, key: String, text: String) -> Self {
        self.callsign = callsign;
        self.group = Some(RadioGroup { key, text });
        self
    }
}

/// Contents of `data/police/radio.toml`. All times are sim seconds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RadioConfig {
    /// Gap after any spoken line before a tactical/routine line, while a racer is wanted, s.
    pub global_gap_s: f32,
    /// Same while no racer is wanted, s.
    pub global_gap_calm_s: f32,
    /// Gap between one unit's tactical/routine lines, s.
    pub unit_gap_s: f32,
    /// Tactical/routine lines wait this long for duplicates from other units to merge, s.
    pub collapse_window_s: f32,
    /// Held tactical/routine lines older than this are dropped, s.
    pub stale_s: f32,
    /// Routine chatter with nobody wanted plays only within this distance of a racer, m.
    pub earshot_m: f32,
}

/// Counters for tuning and tests.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RadioStats {
    /// Lines raised by dispatch logic.
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
    /// Whether any racer has a wanted level.
    pub any_wanted: bool,
    /// Racer ground positions, m.
    pub racers: &'a [[f32; 2]],
    /// Ground position of a unit, m (`None` if gone).
    pub unit_pos: &'a dyn Fn(UnitId) -> Option<[f32; 2]>,
}

/// A spoken line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpokenLine {
    /// Speaking unit (`None` = dispatch, or several units collapsed into one line).
    pub unit: Option<UnitId>,
    /// Text.
    pub text: String,
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
    group: Option<RadioGroup>,
    members: Vec<Member>,
}

/// Holds raised lines and decides which are spoken. See the module docs.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RadioDirector {
    held: Vec<Held>,
    last_line: Option<SimTime>,
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

    /// Raise a line at `now`. It merges into a held line with the same group key.
    pub fn raise(&mut self, now: SimTime, cue: RadioCue) {
        self.stats.raised += 1;
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
            group: cue.group,
            members: vec![member],
        });
    }

    /// Decide what is spoken now. Call once per police step, after all lines were raised.
    pub fn flush(&mut self, cfg: &RadioConfig, ctx: &RadioContext) -> Vec<SpokenLine> {
        let mut spoken = Vec::new();
        if self.held.is_empty() {
            return spoken;
        }
        let now = ctx.now;
        // 1. Critical lines: always, right away.
        let mut k = 0;
        while k < self.held.len() {
            if self.held[k].tier == RadioTier::Critical {
                let h = self.held.remove(k);
                spoken.push(self.speak(now, h));
            } else {
                k += 1;
            }
        }
        // 2. Drop stale lines and members that may not speak; find the best ready line.
        let gap = if ctx.any_wanted {
            cfg.global_gap_s
        } else {
            cfg.global_gap_calm_s
        };
        let gap_open = self.last_line.is_none_or(|t| now.since(t) >= gap);
        let mut best: Option<usize> = None;
        let mut k = 0;
        while k < self.held.len() {
            let age = now.since(self.held[k].raised);
            if age > cfg.stale_s {
                let h = self.held.remove(k);
                self.stats.dropped += h.members.len() as u32;
                continue;
            }
            if age < cfg.collapse_window_s || !gap_open {
                k += 1;
                continue;
            }
            let tier = self.held[k].tier;
            let before = self.held[k].members.len();
            let last_unit = &self.last_unit;
            self.held[k].members.retain(|m| {
                let audible = tier > RadioTier::Routine
                    || ctx.any_wanted
                    || m.unit.and_then(|u| (ctx.unit_pos)(u)).is_some_and(|p| {
                        ctx.racers.iter().any(|r| {
                            let (dx, dy) = (r[0] - p[0], r[1] - p[1]);
                            dx * dx + dy * dy <= cfg.earshot_m * cfg.earshot_m
                        })
                    });
                let rested = m
                    .unit
                    .and_then(|u| last_unit.get(&u))
                    .is_none_or(|t| now.since(*t) >= cfg.unit_gap_s);
                audible && rested
            });
            self.stats.dropped += (before - self.held[k].members.len()) as u32;
            if self.held[k].members.is_empty() {
                self.held.remove(k);
                continue;
            }
            // Higher tier first, then the older line (earlier index = raised earlier).
            if best.is_none_or(|b| tier > self.held[b].tier) {
                best = Some(k);
            }
            k += 1;
        }
        // 3. Speak at most one tactical/routine line per gap.
        if let Some(b) = best {
            let h = self.held.remove(b);
            spoken.push(self.speak(now, h));
        }
        spoken
    }

    fn speak(&mut self, now: SimTime, h: Held) -> SpokenLine {
        self.stats.spoken += 1;
        self.stats.collapsed += h.members.len().saturating_sub(1) as u32;
        self.last_line = Some(now);
        for m in &h.members {
            if let Some(u) = m.unit {
                self.last_unit.insert(u, now);
            }
        }
        if h.members.len() == 1 {
            let m = h.members.into_iter().next().expect("one member");
            return SpokenLine {
                unit: m.unit,
                text: m.text,
            };
        }
        let template = h.group.map(|g| g.text).unwrap_or_default();
        let signs: Vec<String> = h
            .members
            .iter()
            .filter_map(|m| m.callsign.clone())
            .collect();
        SpokenLine {
            unit: None,
            text: template
                .replace("{units}", &join_callsigns(&signs))
                .replace("{count}", &h.members.len().to_string()),
        }
    }
}

/// "Unit 4", "Unit 5", "Unit 7" → "Units 4, 5 and 7"; mixed kinds keep full call signs
/// ("Unit 4 and Air-1").
pub fn join_callsigns(signs: &[String]) -> String {
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
            global_gap_s: 8.0,
            global_gap_calm_s: 20.0,
            unit_gap_s: 30.0,
            collapse_window_s: 1.0,
            stale_s: 6.0,
            earshot_m: 200.0,
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

    fn run(
        d: &mut RadioDirector,
        from: f32,
        to: f32,
        wanted: bool,
        pos: fn(UnitId) -> Option<[f32; 2]>,
    ) -> Vec<(f32, String)> {
        let mut out = Vec::new();
        let mut t = from;
        while t < to {
            let ctx = RadioContext {
                now: SimTime::from_secs(t),
                any_wanted: wanted,
                racers: &[[0.0, 0.0]],
                unit_pos: &pos,
            };
            for l in d.flush(&cfg(), &ctx) {
                out.push((t, l.text));
            }
            t += 0.1;
        }
        out
    }

    #[test]
    fn duplicates_collapse_into_one_line() {
        let mut d = RadioDirector::new();
        for u in [4, 5, 7] {
            d.raise(SimTime::ZERO, patrol(u));
        }
        let out = run(&mut d, 0.0, 3.0, true, near);
        assert_eq!(out.len(), 1, "{out:?}");
        assert_eq!(out[0].1, "Units 4, 5 and 7 resuming patrol.");
        assert_eq!(d.stats().collapsed, 2);
    }

    #[test]
    fn lone_line_keeps_its_own_text() {
        let mut d = RadioDirector::new();
        d.raise(SimTime::ZERO, patrol(4));
        let out = run(&mut d, 0.0, 3.0, true, near);
        assert_eq!(out[0].1, "Unit 4 resuming patrol, Core.");
    }

    #[test]
    fn patrol_chatter_is_off_out_of_earshot_when_nobody_is_wanted() {
        let mut d = RadioDirector::new();
        d.raise(SimTime::ZERO, patrol(4));
        assert!(run(&mut d, 0.0, 10.0, false, far).is_empty());
        assert_eq!(d.stats().dropped, 1);
        d.raise(SimTime::from_secs(10.0), patrol(5));
        assert_eq!(run(&mut d, 10.0, 20.0, false, near).len(), 1);
    }

    #[test]
    fn critical_always_plays_and_routine_waits_for_the_gap() {
        let mut d = RadioDirector::new();
        let crit = |t: &str| RadioCue::new(RadioTier::Critical, None, t.into());
        d.raise(SimTime::ZERO, crit("Dispatch: pursuit."));
        d.raise(SimTime::ZERO, patrol(4));
        d.raise(SimTime::ZERO, crit("Dispatch: roadblock set."));
        let out = run(&mut d, 0.0, 0.05, true, near);
        assert_eq!(
            out.len(),
            2,
            "both critical lines in the same step: {out:?}"
        );
        // The routine line can't play within the 8 s gap and is stale (6 s) before it opens.
        assert!(run(&mut d, 0.1, 20.0, true, near).is_empty());
    }

    #[test]
    fn global_and_unit_gaps_space_lines() {
        let mut d = RadioDirector::new();
        let mut times = Vec::new();
        let mut t = 0.0f32;
        while t < 60.0 {
            // A unit status line every second from a rotating set of 10 units.
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
                any_wanted: true,
                racers: &[[0.0, 0.0]],
                unit_pos: &near,
            };
            for _ in d.flush(&cfg(), &ctx) {
                times.push(t);
            }
            t += 0.1;
        }
        assert!(times.len() <= 8, "{times:?}");
        assert!(times.windows(2).all(|w| w[1] - w[0] >= 7.99), "{times:?}");
    }

    #[test]
    fn callsign_lists() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(join_callsigns(&s(&["Unit 4", "Unit 5"])), "Units 4 and 5");
        assert_eq!(join_callsigns(&s(&["Unit 4", "Air-1"])), "Unit 4 and Air-1");
        assert_eq!(join_callsigns(&s(&["Unit 4"])), "Unit 4");
    }
}
