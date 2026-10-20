//! Siren v2 (playtest row 82): what each police car's siren does, driven by its AI state.
//!
//! The caller (the wasm bridge) tells the director each police car's [`SirenCue`] — a
//! read-only summary of the unit's assignment (`crates/wp_wasm/src/siren_cue.rs`). This module
//! turns cues into the siren the car plays:
//!
//! | cue | siren |
//! |---|---|
//! | `Off` (patrol, idle) | silent |
//! | `Respond` | wail (`two_tone` region: hi-lo) |
//! | `Pursuit` (chasing, target far) | wail (`two_tone`: hi-lo) |
//! | `Close` (chasing, target close) | yelp |
//! | `Junction` (crossing / approaching a junction while running hot) | phaser ("priority") |
//! | `Blocked` (held up behind a vehicle) | yelp + air-horn blasts |
//!
//! plus a short **whoop** chirp when a pursuit starts or a unit gets blocked. A pattern holds
//! for at least `min_hold_s` so the siren does not flap at a cue boundary, and a new pattern
//! starts at the frequency the old one was at (no pitch jump; the backend crossfades the two
//! loop voices). Every unit gets a fixed pitch and sweep-rate spread from its entity id so
//! several cars beat against each other like real ones. At most `max_sirens` loops play, the
//! loudest (at the listener) first.
//!
//! Audio never feeds the simulation: everything here is presentation, deterministic from ticks.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use wp_contracts::{EntityId, PHYSICS_DT};

use crate::data::{SirenConfig, SpatialConfig};
use crate::mixer::{CueRef, Desire};
use crate::siren::{SirenCadence, SirenMode, SirenOutput, SirenPattern};
use crate::siren_space::{AcousticQuery, SirenSource, SirenSpaceFrame, SpaceConfig, SpaceState};
use crate::spatial::{distance_gain, len, sub};
use crate::types::{slot, Bus, Priority, VoiceId};
use crate::SirenOverride;

/// Voice slot of the whoop chirp one-shot (outside the `SIREN..SIREN + 8` loop range and
/// clear of `slot::HORN` = `SIREN + 0x10`).
pub const WHOOP_SLOT: u32 = slot::SIREN + 0x20;
/// Voice slot of an air-horn blast one-shot.
pub const AIRHORN_SLOT: u32 = slot::SIREN + 0x21;
/// Cue name of the whoop chirp in a siren pack.
pub const WHOOP_CUE: &str = "siren/whoop";
/// Cue name of the air-horn blast in a siren pack.
pub const AIRHORN_CUE: &str = "siren/airhorn";

/// What a police car's AI is doing, as far as its siren cares. Read from the police unit by
/// the bridge every frame; never written back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum SirenCue {
    /// Patrol / idle / out of service: siren off.
    Off,
    /// Responding to an incident (or parked at a roadblock / spike strip).
    Respond,
    /// Pursuing or intercepting, target not close.
    Pursuit,
    /// Pursuing, target close.
    Close,
    /// Running hot through or toward a junction.
    Junction,
    /// Held up behind a vehicle: yelp and air horn.
    Blocked,
}

impl SirenCue {
    /// Every cue, in declaration order.
    pub const ALL: [SirenCue; 6] = [
        SirenCue::Off,
        SirenCue::Respond,
        SirenCue::Pursuit,
        SirenCue::Close,
        SirenCue::Junction,
        SirenCue::Blocked,
    ];

    /// Parse `"Off"` / `"Respond"` / `"Pursuit"` / `"Close"` / `"Junction"` / `"Blocked"`.
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| format!("{c:?}") == s)
    }

    /// Whether the unit is chasing someone (a whoop marks entering this state).
    pub fn chasing(self) -> bool {
        matches!(
            self,
            SirenCue::Pursuit | SirenCue::Close | SirenCue::Junction | SirenCue::Blocked
        )
    }
}

/// Siren v2 tuning (`audio_config.json` → `sirens.v2`; every field optional).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SirenV2Config {
    /// Pattern while responding.
    pub respond: SirenPattern,
    /// Pattern while pursuing at a distance.
    pub pursuit: SirenPattern,
    /// Pattern while close to the target.
    pub close: SirenPattern,
    /// Pattern at junctions ("priority").
    pub junction: SirenPattern,
    /// Pattern while blocked (under the air horn).
    pub blocked: SirenPattern,
    /// Regional variant: wail is replaced by the two-tone hi-lo.
    pub two_tone: bool,
    /// Minimum time a pattern plays before another cue may change it, s.
    pub min_hold_s: f32,
    /// Per-unit pitch spread (± fraction of the cadence frequencies).
    pub pitch_spread: f32,
    /// Per-unit sweep-rate spread (± fraction of the cycle length).
    pub tempo_spread: f32,
    /// Whoop chirp gain.
    pub whoop_gain: f32,
    /// Minimum time between two whoops of one unit, s.
    pub whoop_cooldown_s: f32,
    /// Air-horn blast gain.
    pub horn_gain: f32,
    /// Blasts per burst while blocked.
    pub horn_blasts: u32,
    /// Time between blasts of one burst, s.
    pub horn_blast_s: f32,
    /// Time between bursts, s.
    pub horn_period_s: f32,
    /// Delay before the first burst after getting blocked, s.
    pub horn_delay_s: f32,
    /// Siren gain multiplier while a blast sounds (the horn takes over the speaker).
    pub horn_siren_duck: f32,
    /// Length of that duck, s.
    pub horn_duck_s: f32,
    /// Most siren loops playing at once (loudest at the listener win; the focus car always).
    pub max_sirens: usize,
    /// Score bonus for a siren already playing (stops swapping at the limit).
    pub keep_bonus: f32,
    /// Height of the light bar / speaker above the vehicle origin, m.
    pub speaker_height_m: f32,
    /// Acoustic space around the listener (occlusion, slap echo, tunnel, interior).
    pub space: SpaceConfig,
}

impl Default for SirenV2Config {
    fn default() -> Self {
        Self {
            respond: SirenPattern::Wail,
            pursuit: SirenPattern::Wail,
            close: SirenPattern::Yelp,
            junction: SirenPattern::Phaser,
            blocked: SirenPattern::Yelp,
            two_tone: false,
            min_hold_s: 1.2,
            pitch_spread: 0.015,
            tempo_spread: 0.05,
            whoop_gain: 0.9,
            whoop_cooldown_s: 6.0,
            horn_gain: 1.0,
            horn_blasts: 2,
            horn_blast_s: 0.5,
            horn_period_s: 2.6,
            horn_delay_s: 0.3,
            horn_siren_duck: 0.4,
            horn_duck_s: 0.4,
            max_sirens: 4,
            keep_bonus: 1.4,
            speaker_height_m: 1.1,
            space: SpaceConfig::default(),
        }
    }
}

impl SirenV2Config {
    /// Pattern for a cue (`None` = silent).
    pub fn pattern_for(&self, cue: SirenCue) -> Option<SirenPattern> {
        let p = match cue {
            SirenCue::Off => return None,
            SirenCue::Respond => self.respond,
            SirenCue::Pursuit => self.pursuit,
            SirenCue::Close => self.close,
            SirenCue::Junction => self.junction,
            SirenCue::Blocked => self.blocked,
        };
        Some(if self.two_tone && p == SirenPattern::Wail {
            SirenPattern::HiLo
        } else {
            p
        })
    }
}

/// A unit's fixed pitch / sweep-rate spread from its entity id (`(pitch, tempo)` multipliers).
pub fn unit_spread(cfg: &SirenV2Config, entity: u32) -> (f32, f32) {
    // splitmix64: a fixed, platform-independent hash of the id.
    let mut z = (entity as u64).wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    let u = (z & 0xFFFF) as f32 / 65535.0;
    let v = ((z >> 16) & 0xFFFF) as f32 / 65535.0;
    (
        1.0 + cfg.pitch_spread * (2.0 * u - 1.0),
        1.0 + cfg.tempo_spread * (2.0 * v - 1.0),
    )
}

/// Per-vehicle siren v2 state.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SirenUnitState {
    /// Pattern playing, if on.
    pub pattern: Option<SirenPattern>,
    /// Tick the current pattern's phase is counted from.
    pub phase_origin: u64,
    /// Phase at `phase_origin` (a new pattern starts where the old one was: phase matching).
    pub phase_shift: f32,
    /// Tick the current pattern was chosen (hold timer).
    pub since: u64,
    /// Cue seen last frame.
    pub last_cue: Option<SirenCue>,
    /// Frequency and direction last frame (the next pattern starts there).
    pub last_freq: Option<(f32, bool)>,
    /// No whoop before this tick.
    pub whoop_ready: u64,
    /// Next air-horn blast, while blocked.
    pub horn_next: Option<u64>,
    /// Blasts left in the current burst.
    pub horn_left: u32,
    /// Siren ducked under the horn until this tick.
    pub horn_until: u64,
}

/// Everything [`unit_desires`] needs about one vehicle this frame.
#[derive(Debug, Clone, Copy)]
pub struct SirenUnitInput {
    /// Entity id.
    pub entity: u32,
    /// Cue from the bridge (`None` = no police AI behind it: input / mode decide).
    pub cue: Option<SirenCue>,
    /// Testbed override (fixed pattern / off); wins over the cue.
    pub ov: Option<SirenOverride>,
    /// The car's mode when no cue / override applies.
    pub mode: SirenMode,
    /// Siren requested by input (or the unit's assignment).
    pub on: bool,
    /// Speed, m/s (`Auto` mode).
    pub speed_mps: f32,
    /// Simulation tick.
    pub tick: u64,
    /// Speaker position (world), m.
    pub pos: [f32; 3],
    /// Vehicle velocity (world), m/s.
    pub vel: [f32; 3],
    /// Siren loop cue per pattern (`SirenPattern::index`).
    pub loops: [CueRef; 4],
    /// Whoop cue.
    pub whoop: CueRef,
    /// Air-horn cue.
    pub airhorn: CueRef,
}

/// What a unit's siren does this frame (for tests and the debug read-out).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SirenUnitOutput {
    /// The loop: pattern, the unit's (spread) cadence, phase, frequency.
    pub siren: Option<SirenOutput>,
    /// Loop gain multiplier (ducked under the horn).
    pub duck: f32,
    /// A whoop starts this frame.
    pub whoop: bool,
    /// An air-horn blast starts this frame.
    pub horn: bool,
}

fn ticks(s: f32) -> u64 {
    (s.max(0.0) / PHYSICS_DT).round() as u64
}

/// Pattern from an override / the car's mode when there is no cue (pre-v2 behaviour).
fn legacy_pattern(
    sc: &SirenConfig,
    mode: SirenMode,
    on: bool,
    speed: f32,
    cur: Option<SirenPattern>,
) -> Option<SirenPattern> {
    if !on {
        return None;
    }
    match mode {
        SirenMode::Off => None,
        SirenMode::Fixed(p) => Some(p),
        SirenMode::Auto => {
            let speed = speed.abs();
            Some(match cur {
                _ if speed > sc.auto_wail_above_mps => SirenPattern::Wail,
                _ if speed < sc.auto_yelp_below_mps => SirenPattern::Yelp,
                Some(p @ (SirenPattern::Wail | SirenPattern::Yelp)) => p,
                _ => SirenPattern::Wail,
            })
        }
    }
}

/// Advance one unit's siren and decide this frame's sounds.
pub fn update_unit(
    st: &mut SirenUnitState,
    sc: &SirenConfig,
    inp: &SirenUnitInput,
) -> SirenUnitOutput {
    let cfg = &sc.v2;
    let tick = inp.tick;
    let (pitch, tempo) = unit_spread(cfg, inp.entity);
    let cadence_of = |p: SirenPattern| -> Option<SirenCadence> {
        sc.cadence.get(&p).map(|c| c.scaled(pitch, tempo))
    };

    // 1. Which pattern is wanted, and how urgently.
    let (want, cue) = match (inp.ov, inp.cue) {
        (Some(o), _) => (
            legacy_pattern(sc, o.mode, o.on || inp.on, inp.speed_mps, st.pattern),
            None,
        ),
        (None, Some(c)) => (cfg.pattern_for(c), Some(c)),
        (None, None) => (
            legacy_pattern(sc, inp.mode, inp.on, inp.speed_mps, st.pattern),
            None,
        ),
    };

    // 2. Hold: an active pattern only gives way to another after `min_hold_s` (off is instant;
    //    overrides are instant so the lab and the panel respond at once).
    let held = st.pattern.is_some()
        && want.is_some()
        && want != st.pattern
        && inp.ov.is_none()
        && tick < st.since + ticks(cfg.min_hold_s);
    if want != st.pattern && !held {
        match (want, st.last_freq) {
            (Some(p), Some((f, rising))) if st.pattern.is_some() => {
                // Start the new pattern where the old one was (no pitch jump).
                st.phase_origin = tick;
                st.phase_shift = cadence_of(p).map_or(0.0, |c| c.phase_for_freq(f, rising));
            }
            _ => {
                st.phase_origin = tick;
                st.phase_shift = 0.0;
            }
        }
        st.pattern = want;
        st.since = tick;
    }

    // 3. Whoop on a pursuit starting or a unit getting blocked; air horn while blocked.
    let prev = st.last_cue;
    st.last_cue = cue;
    let mut whoop = false;
    if let Some(c) = cue {
        let started_chase = c.chasing() && !prev.is_some_and(|p| p.chasing());
        let got_blocked = c == SirenCue::Blocked && prev != Some(SirenCue::Blocked);
        if (started_chase || got_blocked) && tick >= st.whoop_ready {
            whoop = true;
            st.whoop_ready = tick + ticks(cfg.whoop_cooldown_s);
        }
    }
    let mut horn = false;
    if cue == Some(SirenCue::Blocked) && cfg.horn_blasts > 0 {
        let next = *st.horn_next.get_or_insert_with(|| {
            st.horn_left = cfg.horn_blasts;
            tick + ticks(cfg.horn_delay_s)
        });
        if tick >= next {
            horn = true;
            st.horn_until = tick + ticks(cfg.horn_duck_s);
            st.horn_left = st.horn_left.saturating_sub(1);
            st.horn_next = Some(if st.horn_left == 0 {
                st.horn_left = cfg.horn_blasts;
                tick + ticks(cfg.horn_period_s)
            } else {
                tick + ticks(cfg.horn_blast_s)
            });
        }
    } else {
        st.horn_next = None;
    }

    // 4. The loop.
    let siren = st.pattern.and_then(|p| {
        let cadence = cadence_of(p)?;
        let raw = cadence.phase_after_ticks(tick.saturating_sub(st.phase_origin)) + st.phase_shift;
        let phase = raw - raw.floor();
        Some(SirenOutput {
            pattern: p,
            cadence,
            phase,
            freq_hz: cadence.freq_at_phase(phase),
        })
    });
    st.last_freq = siren.map(|s| (s.freq_hz, s.cadence.rising_at_phase(s.phase)));
    SirenUnitOutput {
        siren,
        duck: if tick < st.horn_until {
            cfg.horn_siren_duck
        } else {
            1.0
        },
        whoop,
        horn,
    }
}

/// Run [`update_unit`] and push the voices it wants.
pub fn unit_desires(
    st: &mut SirenUnitState,
    sc: &SirenConfig,
    inp: &SirenUnitInput,
    out: &mut Vec<Desire>,
) -> SirenUnitOutput {
    let o = update_unit(st, sc, inp);
    let e = inp.entity;
    if let Some(so) = o.siren {
        let mut d = Desire::new(
            VoiceId::new(e, slot::SIREN + so.pattern.index()),
            Bus::Police,
            inp.loops[so.pattern.index() as usize],
            Priority::High,
            inp.pos,
            inp.vel,
        );
        d.gain = sc.gain * o.duck;
        d.freq_hz = so.freq_hz;
        d.phase = so.phase;
        let mut synth = so.cadence.synth_params();
        // The backend's per-unit seed (stable detune of its harmonic driver, drift).
        synth.insert("unit".into(), (e % 997) as f32);
        d.synth = Some(synth);
        out.push(d);
    }
    for (fire, slot_code, cue, gain) in [
        (o.whoop, WHOOP_SLOT, inp.whoop, sc.v2.whoop_gain),
        (o.horn, AIRHORN_SLOT, inp.airhorn, sc.v2.horn_gain),
    ] {
        if fire {
            let mut d = Desire::new(
                VoiceId::new(e, slot_code),
                Bus::Police,
                cue,
                Priority::High,
                inp.pos,
                inp.vel,
            );
            d.gain = sc.gain * gain;
            let (pitch, _) = unit_spread(&sc.v2, e);
            d.pitch = pitch;
            out.push(d);
        }
    }
    o
}

/// Whether a voice slot belongs to the siren system (loops, whoop, air horn).
pub fn is_siren_slot(s: u32) -> bool {
    (slot::SIREN..slot::SIREN + 4).contains(&s) || s == WHOOP_SLOT || s == AIRHORN_SLOT
}

/// Director-side siren v2 state shared by every vehicle: cues from the bridge, which sirens
/// won the `max_sirens` limit, and the acoustic space around the listener.
#[derive(Debug, Clone, Default)]
pub struct SirenRack {
    /// Cues set by the bridge this frame.
    pub cues: BTreeMap<EntityId, SirenCue>,
    /// Entities whose siren loop played last frame.
    playing: BTreeSet<u32>,
    /// Siren loops kept this frame (for the space pass).
    sources: Vec<SirenSource>,
    /// Focus entity of this frame.
    focus: Option<u32>,
    /// Tick of this frame.
    tick: u64,
    space: SpaceState,
}

impl SirenRack {
    /// Set (or clear) the cue of `entity`. Cues of vehicles that leave are forgotten.
    pub fn set_cue(&mut self, entity: EntityId, cue: Option<SirenCue>) {
        match cue {
            Some(c) => {
                self.cues.insert(entity, c);
            }
            None => {
                self.cues.remove(&entity);
            }
        }
    }

    /// Forget everything (session reset).
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// Drop cues of vehicles not in this frame.
    pub fn retain(&mut self, present: &BTreeSet<EntityId>) {
        self.cues.retain(|e, _| present.contains(e));
    }

    /// Keep at most `max_sirens` siren loops (the loudest at the listener, the focus car always,
    /// a bonus for those already playing); drop the rest together with their whoops and horns.
    pub fn limit(
        &mut self,
        cfg: &SirenV2Config,
        spatial: &SpatialConfig,
        listener_pos: [f32; 3],
        focus: Option<EntityId>,
        tick: u64,
        desires: &mut Vec<Desire>,
    ) {
        self.focus = focus.map(|f| f.0);
        self.tick = tick;
        let mut scored: Vec<(f32, u32)> = desires
            .iter()
            .filter(|d| (slot::SIREN..slot::SIREN + 4).contains(&d.id.slot))
            .map(|d| {
                let g = d.gain * distance_gain(spatial, len(sub(d.pos, listener_pos)));
                let keep = if self.playing.contains(&d.id.entity) {
                    cfg.keep_bonus
                } else {
                    1.0
                };
                let focus_bonus = if self.focus == Some(d.id.entity) {
                    1e6
                } else {
                    1.0
                };
                (g * keep * focus_bonus, d.id.entity)
            })
            .collect();
        scored.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
        let kept: BTreeSet<u32> = scored
            .iter()
            .take(cfg.max_sirens)
            .map(|(_, e)| *e)
            .collect();
        let dropped: BTreeSet<u32> = scored
            .iter()
            .skip(cfg.max_sirens)
            .map(|(_, e)| *e)
            .collect();
        desires.retain(|d| !(is_siren_slot(d.id.slot) && dropped.contains(&d.id.entity)));
        self.sources = desires
            .iter()
            .filter(|d| (slot::SIREN..slot::SIREN + 4).contains(&d.id.slot))
            .map(|d| SirenSource {
                entity: d.id.entity,
                pattern: SirenPattern::ALL[(d.id.slot - slot::SIREN) as usize],
                pos: [d.pos[0], d.pos[1], d.pos[2] + cfg.speaker_height_m],
                gain: d.gain * distance_gain(spatial, len(sub(d.pos, listener_pos))),
            })
            .collect();
        self.playing = kept;
    }

    /// Entities whose siren loop plays (after the limit).
    pub fn playing(&self) -> &BTreeSet<u32> {
        &self.playing
    }

    /// The acoustic pass for this frame's sirens: occlusion, interior muffle, street slap echo,
    /// tunnel reverb and the engine duck. `cabin` = the vehicle whose cabin the listener is in. `query` answers line-of-sight and "covered" questions
    /// about the world (use [`crate::siren_space::OpenAir`] without a city).
    pub fn space(
        &mut self,
        cfg: &SirenV2Config,
        listener: &crate::types::Listener,
        cabin: Option<u32>,
        query: &dyn AcousticQuery,
    ) -> SirenSpaceFrame {
        self.space.set_cabin(cabin);
        self.space.update(
            &cfg.space,
            listener,
            &self.sources,
            self.focus,
            self.tick,
            query,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::siren::SirenShape;

    fn sc() -> SirenConfig {
        let sweep = |lo, hi, cycle_s, rise_frac| SirenCadence {
            lo_hz: lo,
            hi_hz: hi,
            cycle_s,
            rise_frac,
            shape: SirenShape::Sweep,
        };
        let mut cadence = BTreeMap::new();
        cadence.insert(SirenPattern::Wail, sweep(650.0, 1600.0, 4.0, 0.6));
        cadence.insert(SirenPattern::Yelp, sweep(650.0, 1600.0, 0.3, 0.5));
        cadence.insert(SirenPattern::Phaser, sweep(700.0, 1700.0, 0.1, 0.85));
        cadence.insert(
            SirenPattern::HiLo,
            SirenCadence {
                lo_hz: 700.0,
                hi_hz: 920.0,
                cycle_s: 1.0,
                rise_frac: 0.5,
                shape: SirenShape::TwoTone,
            },
        );
        SirenConfig {
            cadence,
            auto_wail_above_mps: 16.0,
            auto_yelp_below_mps: 9.0,
            gain: 1.0,
            horn_gain: 1.0,
            v2: SirenV2Config::default(),
        }
    }

    fn input(entity: u32, cue: Option<SirenCue>, tick: u64) -> SirenUnitInput {
        SirenUnitInput {
            entity,
            cue,
            ov: None,
            mode: SirenMode::Auto,
            on: cue.is_some_and(|c| c != SirenCue::Off),
            speed_mps: 20.0,
            tick,
            pos: [0.0; 3],
            vel: [0.0; 3],
            loops: [CueRef(0), CueRef(1), CueRef(2), CueRef(3)],
            whoop: CueRef(4),
            airhorn: CueRef(5),
        }
    }

    #[test]
    fn cues_pick_the_documented_patterns() {
        let c = SirenV2Config::default();
        assert_eq!(c.pattern_for(SirenCue::Off), None);
        assert_eq!(c.pattern_for(SirenCue::Respond), Some(SirenPattern::Wail));
        assert_eq!(c.pattern_for(SirenCue::Close), Some(SirenPattern::Yelp));
        assert_eq!(
            c.pattern_for(SirenCue::Junction),
            Some(SirenPattern::Phaser)
        );
        assert_eq!(c.pattern_for(SirenCue::Blocked), Some(SirenPattern::Yelp));
        let eu = SirenV2Config {
            two_tone: true,
            ..Default::default()
        };
        assert_eq!(eu.pattern_for(SirenCue::Respond), Some(SirenPattern::HiLo));
        assert_eq!(SirenCue::parse("Junction"), Some(SirenCue::Junction));
    }

    #[test]
    fn whoop_on_pursuit_start_and_horn_bursts_while_blocked() {
        let sc = sc();
        let mut st = SirenUnitState::default();
        let mut whoops = Vec::new();
        let mut horns = Vec::new();
        let script = |t: u64| match t {
            0..240 => SirenCue::Off,
            240..1200 => SirenCue::Respond,
            1200..2400 => SirenCue::Pursuit,
            2400..4800 => SirenCue::Blocked,
            _ => SirenCue::Close,
        };
        for t in 0..6000u64 {
            let o = update_unit(&mut st, &sc, &input(3, Some(script(t)), t));
            if o.whoop {
                whoops.push(t);
            }
            if o.horn {
                horns.push(t);
                assert!(o.duck < 1.0, "siren ducks under the horn");
            }
            if t < 240 {
                assert!(o.siren.is_none(), "patrol is silent");
            }
        }
        // Pursuit start (1200) whoops; getting blocked 5 s later is inside the 6 s cooldown.
        assert_eq!(whoops, vec![1200]);
        // Blocked 10 s: bursts of 2 blasts (0.5 s apart) every 2.6 s after a 0.3 s delay.
        assert!(horns.len() >= 6, "{horns:?}");
        assert_eq!(horns[0], 2400 + 72);
        assert_eq!(horns[1] - horns[0], 120);
        assert!(horns.iter().all(|t| (2400..4800).contains(t)));
    }

    #[test]
    fn pattern_holds_then_switches_without_a_pitch_jump() {
        let sc = sc();
        let mut st = SirenUnitState::default();
        let mut last: Option<SirenOutput> = None;
        let mut switches = 0;
        for t in 0..2400u64 {
            // The cue flaps every 0.25 s between Pursuit and Junction.
            let cue = if (t / 60) % 2 == 0 {
                SirenCue::Pursuit
            } else {
                SirenCue::Junction
            };
            let o = update_unit(&mut st, &sc, &input(9, Some(cue), t))
                .siren
                .unwrap();
            if let Some(l) = last {
                if l.pattern != o.pattern {
                    switches += 1;
                    let rel = (o.freq_hz - l.freq_hz).abs() / l.freq_hz;
                    assert!(rel < 0.08, "switch jumped {} → {} Hz", l.freq_hz, o.freq_hz);
                }
            }
            last = Some(o);
        }
        // 10 s at a 1.2 s hold: at most ~8 switches instead of 40.
        assert!((2..=9).contains(&switches), "{switches} switches");
    }

    #[test]
    fn one_shot_slots_do_not_collide() {
        for s in [WHOOP_SLOT, AIRHORN_SLOT] {
            assert_ne!(s, slot::HORN);
            assert!(!(slot::SIREN..slot::SIREN + 8).contains(&s));
            assert!(!slot::is_one_shot(s));
        }
    }

    #[test]
    fn units_are_spread_in_pitch_and_tempo() {
        let cfg = SirenV2Config::default();
        let a = unit_spread(&cfg, 101);
        let b = unit_spread(&cfg, 102);
        assert_ne!(a, b);
        for e in 0..500 {
            let (p, t) = unit_spread(&cfg, e);
            assert!((p - 1.0).abs() <= cfg.pitch_spread + 1e-6);
            assert!((t - 1.0).abs() <= cfg.tempo_spread + 1e-6);
        }
        assert_eq!(unit_spread(&cfg, 7), unit_spread(&cfg, 7), "stable");
    }

    #[test]
    fn override_wins_and_legacy_auto_still_works() {
        let sc = sc();
        let mut st = SirenUnitState::default();
        let mut inp = input(1, Some(SirenCue::Respond), 0);
        inp.ov = Some(SirenOverride {
            on: true,
            mode: SirenMode::Fixed(SirenPattern::HiLo),
        });
        let o = update_unit(&mut st, &sc, &inp).siren.unwrap();
        assert_eq!(o.pattern, SirenPattern::HiLo);
        let mut st = SirenUnitState::default();
        let mut inp = input(1, None, 0);
        inp.on = true;
        inp.speed_mps = 3.0;
        assert_eq!(
            update_unit(&mut st, &sc, &inp).siren.unwrap().pattern,
            SirenPattern::Yelp
        );
    }

    #[test]
    fn limit_keeps_the_loudest_and_the_focus() {
        let spatial = SpatialConfig {
            ref_distance_m: 6.0,
            max_distance_m: 450.0,
            rolloff: 1.0,
            speed_of_sound_mps: 343.0,
            doppler_factor: 1.0,
            max_doppler: 1.6,
            lowpass_near_hz: 18000.0,
            lowpass_half_distance_m: 90.0,
        };
        let cfg = SirenV2Config {
            max_sirens: 2,
            ..Default::default()
        };
        let mk = |e: u32, d: f32, s: u32| {
            Desire::new(
                VoiceId::new(e, s),
                Bus::Police,
                CueRef(0),
                Priority::High,
                [d, 0.0, 0.0],
                [0.0; 3],
            )
        };
        let mut desires = vec![
            mk(1, 300.0, slot::SIREN),
            mk(2, 20.0, slot::SIREN + 1),
            mk(3, 50.0, slot::SIREN),
            mk(3, 50.0, WHOOP_SLOT),
            mk(4, 10.0, slot::SIREN),
            mk(1, 300.0, AIRHORN_SLOT),
        ];
        let mut rack = SirenRack::default();
        rack.limit(&cfg, &spatial, [0.0; 3], Some(EntityId(1)), 0, &mut desires);
        let kept: Vec<(u32, u32)> = desires.iter().map(|d| (d.id.entity, d.id.slot)).collect();
        // Focus (1, far away) and the loudest other (4) stay; 2 and 3 (and 3's whoop) go.
        assert_eq!(
            kept,
            vec![(1, slot::SIREN), (4, slot::SIREN), (1, AIRHORN_SLOT)]
        );
        assert_eq!(
            rack.playing().iter().copied().collect::<Vec<_>>(),
            vec![1, 4]
        );
    }
}
