//! Radio scanner: police radio lines are queued by priority, released after a delay set by the
//! listener's scanner tier (better scanners hear dispatch sooner, 3–10 s), spoken one at a
//! time, and duck the music bus while a line plays. A more urgent line chops a less urgent
//! one mid-sentence.
//!
//! Lines come from `wp_police::PoliceEvent::Radio { unit, text }` (decoded from
//! `SimEvent::PoliceEvent` by the director) or [`crate::AudioDirector::push_radio_line`]. The
//! police payload has no priority yet (request A-2): the director uses Dispatch for dispatch
//! lines, Pursuit while any pursuit is active, Routine otherwise. The listener's tier is set by
//! the caller (the item lives with the player's gadgets).

use serde::{Deserialize, Serialize};
use wp_contracts::PHYSICS_DT;

use crate::types::{AudioCommand, Bus};

/// Priority of a radio line.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default,
)]
pub enum RadioPriority {
    /// Background chatter (unit status, patrol).
    #[default]
    Chatter,
    /// Routine dispatch (respond to incident).
    Routine,
    /// Assignment to a pursuit, roadblock or spike strip.
    Dispatch,
    /// Live pursuit callouts (direction, speed, target vehicle).
    Pursuit,
    /// Officer down, helicopter on target, shots — always interrupts.
    Urgent,
}

/// Scanner quality owned by the listener. `Basic`/`Pro`/`Elite` are `wp_police`'s scanner tiers
/// 0/1/2 (`data/police/tools.toml` `scanner_delay_s`; the audio config mirrors those delays and a
/// test keeps them equal).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ScannerTier {
    /// No scanner: lines are not heard.
    None,
    /// Police tier 0 (longest delay).
    Basic,
    /// Police tier 1.
    Pro,
    /// Police tier 2 (shortest delay).
    Elite,
}

impl ScannerTier {
    /// Parse `"None" | "Basic" | "Pro" | "Elite"`.
    pub fn parse(s: &str) -> Option<Self> {
        [Self::None, Self::Basic, Self::Pro, Self::Elite]
            .into_iter()
            .find(|t| format!("{t:?}") == s)
    }

    /// `wp_police` tier index (`PoliceSystem::scanner(tier, ..)`), `None` without a scanner.
    pub fn police_index(self) -> Option<usize> {
        match self {
            ScannerTier::None => None,
            ScannerTier::Basic => Some(0),
            ScannerTier::Pro => Some(1),
            ScannerTier::Elite => Some(2),
        }
    }
}

/// Scanner tuning.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScannerConfig {
    /// Delay before a line is heard for Basic, Pro, Elite, s (= `wp_police` `scanner_delay_s`).
    pub tier_delay_s: [f32; 3],
    /// Maximum queued lines; the lowest-priority oldest is dropped beyond this.
    pub max_queue: usize,
    /// A released line not spoken within this time is dropped (stale), s.
    pub stale_after_s: f32,
    /// Speaking rate for duration estimates, words/s.
    pub words_per_s: f32,
    /// Squelch / pause added to every line, s.
    pub line_pad_s: f32,
    /// Music duck multiplier while a line plays.
    pub music_duck_gain: f32,
    /// Duck ramp, s.
    pub duck_fade_s: f32,
    /// Restore ramp, s.
    pub restore_fade_s: f32,
    /// Lines at or above this priority interrupt lower-priority lines.
    pub interrupt_at: RadioPriority,
}

impl ScannerConfig {
    /// Delay for a tier, s (`None` for no scanner).
    pub fn delay_s(&self, tier: ScannerTier) -> Option<f32> {
        tier.police_index().map(|i| self.tier_delay_s[i])
    }
}

/// One radio line as emitted by police.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RadioLine {
    /// Speaking unit (police entity id / callsign number).
    pub unit: u32,
    /// Text.
    pub text: String,
    /// Priority.
    pub priority: RadioPriority,
    /// Tick the police system emitted it.
    pub emitted_tick: u64,
}

#[derive(Debug, Clone, PartialEq)]
struct Queued {
    seq: u32,
    line: RadioLine,
    release_tick: u64,
}

#[derive(Debug, Clone, PartialEq)]
struct Speaking {
    seq: u32,
    priority: RadioPriority,
    end_tick: u64,
}

/// The scanner queue.
#[derive(Debug, Clone, PartialEq)]
pub struct Scanner {
    tier: ScannerTier,
    queue: Vec<Queued>,
    speaking: Option<Speaking>,
    next_seq: u32,
    /// Lines dropped (no scanner, overflow, stale) — debug counter.
    pub dropped: u32,
}

impl Default for Scanner {
    fn default() -> Self {
        Self {
            tier: ScannerTier::Basic,
            queue: Vec::new(),
            speaking: None,
            next_seq: 1,
            dropped: 0,
        }
    }
}

/// Estimated speaking time of `text`, s.
pub fn line_duration_s(cfg: &ScannerConfig, text: &str) -> f32 {
    let words = text.split_whitespace().count().max(1) as f32;
    words / cfg.words_per_s.max(0.5) + cfg.line_pad_s
}

/// Per-unit voice pitch in `0.85..1.15`, from a fixed integer hash (stable everywhere).
pub fn unit_pitch(unit: u32) -> f32 {
    let h = unit.wrapping_mul(2_654_435_761) >> 16;
    0.85 + (h % 1000) as f32 / 1000.0 * 0.3
}

fn ticks(s: f32) -> u64 {
    (s.max(0.0) / PHYSICS_DT).round() as u64
}

impl Scanner {
    /// Current tier.
    pub fn tier(&self) -> ScannerTier {
        self.tier
    }

    /// Change the tier (applies to lines queued from now on).
    pub fn set_tier(&mut self, tier: ScannerTier) {
        self.tier = tier;
    }

    /// Number of queued (not yet spoken) lines.
    pub fn queued(&self) -> usize {
        self.queue.len()
    }

    /// Queue a line. Without a scanner the line is dropped.
    pub fn push(&mut self, cfg: &ScannerConfig, line: RadioLine) {
        let Some(delay) = cfg.delay_s(self.tier) else {
            self.dropped += 1;
            return;
        };
        let release_tick = line.emitted_tick + ticks(delay);
        self.queue.push(Queued {
            seq: self.next_seq,
            line,
            release_tick,
        });
        self.next_seq += 1;
        while self.queue.len() > cfg.max_queue.max(1) {
            // Drop the lowest priority; among equals the oldest.
            let worst = (0..self.queue.len())
                .min_by(|&a, &b| {
                    let (qa, qb) = (&self.queue[a], &self.queue[b]);
                    qa.line
                        .priority
                        .cmp(&qb.line.priority)
                        .then(qa.seq.cmp(&qb.seq))
                })
                .expect("non-empty");
            self.queue.remove(worst);
            self.dropped += 1;
        }
    }

    /// Advance to `tick`, pushing radio and duck commands.
    pub fn update(&mut self, cfg: &ScannerConfig, tick: u64, out: &mut Vec<AudioCommand>) {
        // Stale lines go.
        let stale = ticks(cfg.stale_after_s);
        let before = self.queue.len();
        self.queue.retain(|q| tick <= q.release_tick + stale);
        self.dropped += (before - self.queue.len()) as u32;

        // Best released line: highest priority, then earliest release, then sequence.
        let best = (0..self.queue.len())
            .filter(|&i| self.queue[i].release_tick <= tick)
            .max_by(|&a, &b| {
                let (qa, qb) = (&self.queue[a], &self.queue[b]);
                qa.line
                    .priority
                    .cmp(&qb.line.priority)
                    .then(qb.release_tick.cmp(&qa.release_tick))
                    .then(qb.seq.cmp(&qa.seq))
            });

        if let Some(sp) = &self.speaking {
            let interrupt = best.is_some_and(|i| {
                let p = self.queue[i].line.priority;
                p >= cfg.interrupt_at && p > sp.priority
            });
            if tick >= sp.end_tick || interrupt {
                out.push(AudioCommand::RadioEnd {
                    line: sp.seq,
                    interrupted: tick < sp.end_tick,
                });
                self.speaking = None;
                if best.is_none() {
                    out.push(AudioCommand::Duck {
                        bus: Bus::Music,
                        gain: 1.0,
                        fade_s: cfg.restore_fade_s,
                    });
                }
            } else {
                return;
            }
        }
        let Some(i) = best else {
            return;
        };
        let q = self.queue.remove(i);
        let duration_s = line_duration_s(cfg, &q.line.text);
        out.push(AudioCommand::Duck {
            bus: Bus::Music,
            gain: cfg.music_duck_gain,
            fade_s: cfg.duck_fade_s,
        });
        out.push(AudioCommand::RadioSay {
            line: q.seq,
            unit: q.line.unit,
            text: q.line.text.clone(),
            priority: q.line.priority,
            duration_s,
            voice_pitch: unit_pitch(q.line.unit),
        });
        self.speaking = Some(Speaking {
            seq: q.seq,
            priority: q.line.priority,
            end_tick: tick + ticks(duration_s).max(1),
        });
    }

    /// Drop everything (session reset).
    pub fn clear(&mut self, out: &mut Vec<AudioCommand>) {
        if let Some(sp) = self.speaking.take() {
            out.push(AudioCommand::RadioEnd {
                line: sp.seq,
                interrupted: true,
            });
            out.push(AudioCommand::Duck {
                bus: Bus::Music,
                gain: 1.0,
                fade_s: 0.1,
            });
        }
        self.queue.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> ScannerConfig {
        ScannerConfig {
            tier_delay_s: [10.0, 6.0, 3.0],
            max_queue: 4,
            stale_after_s: 20.0,
            words_per_s: 2.5,
            line_pad_s: 0.6,
            music_duck_gain: 0.3,
            duck_fade_s: 0.15,
            restore_fade_s: 0.6,
            interrupt_at: RadioPriority::Pursuit,
        }
    }

    fn line(text: &str, p: RadioPriority, tick: u64) -> RadioLine {
        RadioLine {
            unit: 7,
            text: text.into(),
            priority: p,
            emitted_tick: tick,
        }
    }

    fn says(out: &[AudioCommand]) -> Vec<String> {
        out.iter()
            .filter_map(|c| match c {
                AudioCommand::RadioSay { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn delay_by_tier() {
        for (tier, delay) in [
            (ScannerTier::Basic, 10.0),
            (ScannerTier::Pro, 6.0),
            (ScannerTier::Elite, 3.0),
        ] {
            let c = cfg();
            let mut s = Scanner::default();
            s.set_tier(tier);
            s.push(&c, line("unit seven responding", RadioPriority::Routine, 0));
            let mut out = Vec::new();
            let release = ticks(delay);
            s.update(&c, release - 1, &mut out);
            assert!(says(&out).is_empty(), "{tier:?} too early");
            s.update(&c, release, &mut out);
            assert_eq!(says(&out).len(), 1, "{tier:?} on time");
        }
    }

    #[test]
    fn no_scanner_hears_nothing() {
        let c = cfg();
        let mut s = Scanner::default();
        s.set_tier(ScannerTier::None);
        s.push(&c, line("x", RadioPriority::Urgent, 0));
        let mut out = Vec::new();
        s.update(&c, 100_000, &mut out);
        assert!(out.is_empty());
        assert_eq!(s.dropped, 1);
    }

    #[test]
    fn priority_order_and_one_at_a_time() {
        let c = cfg();
        let mut s = Scanner::default();
        s.set_tier(ScannerTier::Elite);
        s.push(&c, line("chatter one", RadioPriority::Chatter, 0));
        s.push(&c, line("dispatch two", RadioPriority::Dispatch, 0));
        let mut out = Vec::new();
        let mut t = ticks(3.0);
        s.update(&c, t, &mut out);
        assert_eq!(says(&out), vec!["dispatch two"]);
        assert!(matches!(
            out[0],
            AudioCommand::Duck {
                bus: Bus::Music,
                ..
            }
        ));
        out.clear();
        // Still speaking: nothing new.
        s.update(&c, t + 10, &mut out);
        assert!(out.is_empty());
        t += ticks(line_duration_s(&c, "dispatch two")) + 1;
        s.update(&c, t, &mut out);
        assert_eq!(says(&out), vec!["chatter one"]);
    }

    #[test]
    fn urgent_line_chops_chatter() {
        let c = cfg();
        let mut s = Scanner::default();
        s.set_tier(ScannerTier::Elite);
        s.push(
            &c,
            line(
                "a long bit of chatter about nothing much at all",
                RadioPriority::Chatter,
                0,
            ),
        );
        let mut out = Vec::new();
        s.update(&c, ticks(3.0), &mut out);
        s.push(
            &c,
            line("suspect heading north", RadioPriority::Pursuit, ticks(1.0)),
        );
        out.clear();
        s.update(&c, ticks(4.0), &mut out);
        assert!(out.iter().any(|c| matches!(
            c,
            AudioCommand::RadioEnd {
                interrupted: true,
                ..
            }
        )));
        assert_eq!(says(&out), vec!["suspect heading north"]);
    }

    #[test]
    fn overflow_drops_lowest_priority() {
        let c = cfg();
        let mut s = Scanner::default();
        for i in 0..4 {
            s.push(&c, line(&format!("routine {i}"), RadioPriority::Routine, 0));
        }
        s.push(&c, line("chatter", RadioPriority::Chatter, 0));
        assert_eq!(s.queued(), 4);
        assert_eq!(s.dropped, 1);
        assert!(s
            .queue
            .iter()
            .all(|q| q.line.priority == RadioPriority::Routine));
    }

    #[test]
    fn unit_pitch_in_range_and_varies() {
        let a = unit_pitch(1);
        let b = unit_pitch(2);
        assert!((0.85..=1.15).contains(&a) && (0.85..=1.15).contains(&b));
        assert_ne!(a, b);
    }
}
