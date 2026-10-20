//! The output vocabulary of `wp_audio`: voices, buses, priorities and [`AudioCommand`]s.
//!
//! A *voice* is one playing sound with a stable [`VoiceId`]. Looped voices (engine layers,
//! tire squeal, sirens, scrapes) live across frames and receive [`AudioCommand::Set`] updates;
//! one-shots (impacts, thumps, blow-off) are a single [`AudioCommand::Play`] with
//! `looped = false`. The backend never decides *what* plays, only *how* (samples or synth).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Mixer bus. The backend owns one gain node per bus (with debug sliders).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default,
)]
pub enum Bus {
    /// Engine, induction and exhaust layers.
    #[default]
    Engine,
    /// Tires, surfaces, suspension, impacts, scrapes, debris, horns.
    World,
    /// Sirens and the radio scanner.
    Police,
    /// Interface sounds (not produced by the director yet).
    Ui,
    /// Music (only ever ducked by the director).
    Music,
}

impl Bus {
    /// Every bus, in a fixed order.
    pub const ALL: [Bus; 5] = [Bus::Engine, Bus::World, Bus::Police, Bus::Ui, Bus::Music];
}

/// Voice priority class. The culling score is `priority weight × audible gain`
/// (weights in [`crate::AudioConfig::priority_weights`]).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default,
)]
pub enum Priority {
    /// Ambient detail (distant rolling noise, suspension ticks).
    Low,
    /// Ordinary gameplay sound.
    #[default]
    Normal,
    /// Important cues (impacts, sirens).
    High,
    /// Never culled while audible (the focused car's engine, glass shatter).
    Critical,
}

impl Priority {
    /// Index into [`crate::AudioConfig::priority_weights`].
    pub fn index(self) -> usize {
        self as usize
    }
}

/// Stable identity of a voice: owning entity plus a slot code (see [`slot`]).
/// Serialized as `{ "entity": u32, "slot": u32 }` so it is exact in JavaScript.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default,
)]
pub struct VoiceId {
    /// Owning entity (`u32::MAX` = the world / no entity).
    pub entity: u32,
    /// Slot code within the entity; constants and constructors in [`slot`].
    pub slot: u32,
}

impl VoiceId {
    /// Voice `slot` on `entity`.
    pub const fn new(entity: u32, slot: u32) -> Self {
        Self { entity, slot }
    }
}

/// Slot codes for [`VoiceId::slot`]. Looped voices use fixed codes; one-shots and scrapes
/// encode a sequence number / the other entity in the low bits.
pub mod slot {
    /// Engine layer `i` (index into the pack's `engine_layers`): `ENGINE_LAYER + i`.
    pub const ENGINE_LAYER: u32 = 0x0000_0100;
    /// Rev-limiter layer.
    pub const ENGINE_LIMITER: u32 = 0x0000_0180;
    /// Turbo whistle loop.
    pub const TURBO: u32 = 0x0000_0190;
    /// Supercharger whine loop.
    pub const SUPERCHARGER: u32 = 0x0000_0191;
    /// Cooling-damage steam hiss loop.
    pub const STEAM: u32 = 0x0000_01A0;
    /// Loose / damaged body panels rattling over bumps.
    pub const BODY_RATTLE: u32 = 0x0000_01A1;
    /// Granular crunch loop following the lattice's deformation power (`impact/crunch`).
    pub const CRUNCH: u32 = 0x0000_01A2;
    /// Tire squeal loop (paved surfaces).
    pub const SQUEAL: u32 = 0x0000_0200;
    /// Loose-surface scrub loop (gravel, dirt, grass).
    pub const SCRUB: u32 = 0x0000_0201;
    /// Driven-wheel spin loop (paved surfaces).
    pub const SPIN: u32 = 0x0000_0202;
    /// Driven-wheel spin loop (loose surfaces).
    pub const SPIN_LOOSE: u32 = 0x0000_0203;
    /// Surface rolling noise for `SurfaceType` index `i`: `SURFACE + i`.
    pub const SURFACE: u32 = 0x0000_0210;
    /// Siren loop for pattern index `i` (`SirenPattern::index`): `SIREN + i`.
    pub const SIREN: u32 = 0x0000_0300;
    /// Horn loop.
    pub const HORN: u32 = 0x0000_0310;
    /// Scrape loop against another entity: `SCRAPE | (other & 0x3FFF_FFFF)`.
    pub const SCRAPE: u32 = 0x4000_0000;
    /// One-shot number `n`: `ONE_SHOT | (n & 0x7FFF_FFFF)`.
    pub const ONE_SHOT: u32 = 0x8000_0000;

    /// Scrape slot for contact with `other`.
    pub const fn scrape(other: u32) -> u32 {
        SCRAPE | (other & 0x3FFF_FFFF)
    }

    /// One-shot slot for sequence number `n`.
    pub const fn one_shot(n: u32) -> u32 {
        ONE_SHOT | (n & 0x7FFF_FFFF)
    }

    /// Whether a slot code is a one-shot.
    pub const fn is_one_shot(slot: u32) -> bool {
        slot & ONE_SHOT != 0
    }
}

/// Per-frame parameters of a voice. Every value is final: distance attenuation and Doppler are
/// already folded in, so the backend only pans (direction from `pos`) and applies bus gains.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub struct VoiceParams {
    /// Linear gain including distance attenuation (before bus gain), 0..≈1.
    pub gain: f32,
    /// Playback-rate multiplier for sample sources (rpm / root rpm, × Doppler).
    pub pitch: f32,
    /// Fundamental frequency for procedural synths, Hz (× Doppler). 0 = not applicable.
    pub freq_hz: f32,
    /// Synth brightness / engine load / slip intensity, 0..=1.
    pub load: f32,
    /// Cycle phase 0..1 for periodic synths (sirens); 0 otherwise.
    pub phase: f32,
    /// Air-absorption low-pass cutoff from distance, Hz.
    pub lowpass_hz: f32,
    /// Doppler ratio that was folded into `pitch` and `freq_hz` (debug display).
    pub doppler: f32,
    /// Source position in world space, m (for the backend's panner).
    pub pos: [f32; 3],
}

/// One instruction for the audio backend. Serialized externally tagged
/// (`{"Play": {...}}`) — the wasm surface ships them to TypeScript as JSON.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AudioCommand {
    /// Start a voice (looped voices stay until [`AudioCommand::Stop`]; one-shots end alone).
    Play {
        /// Voice identity.
        voice: VoiceId,
        /// Mixer bus.
        bus: Bus,
        /// Sample pack id (`data/audio/packs/<pack>.json`).
        pack: String,
        /// Cue name inside the pack.
        cue: String,
        /// Loop until stopped.
        looped: bool,
        /// Priority class it won its slot with.
        priority: Priority,
        /// Initial parameters.
        params: VoiceParams,
        /// Per-car synth parameters (cylinders, firing unevenness, …) merged over the cue's
        /// procedural params. Empty for most voices.
        synth: BTreeMap<String, f32>,
    },
    /// Update a running voice.
    Set {
        /// Voice identity.
        voice: VoiceId,
        /// New parameters (the backend ramps to them over one frame).
        params: VoiceParams,
    },
    /// Fade out and release a voice.
    Stop {
        /// Voice identity.
        voice: VoiceId,
        /// Fade-out time, s.
        fade_s: f32,
    },
    /// Duck (or restore) a bus: multiply its user gain by `gain` with a ramp of `fade_s`.
    Duck {
        /// Bus to duck.
        bus: Bus,
        /// Duck multiplier, 0..=1 (1 = restored).
        gain: f32,
        /// Ramp time, s.
        fade_s: f32,
    },
    /// Speak one radio-scanner line (squelch on, radio EQ, TTS or recorded clip).
    RadioSay {
        /// Line id (unique per director).
        line: u32,
        /// Speaking unit (police entity id or callsign number).
        unit: u32,
        /// Text to speak.
        text: String,
        /// Line priority.
        priority: crate::RadioPriority,
        /// Time the director reserves for the line, s.
        duration_s: f32,
        /// Voice pitch multiplier derived from the unit (each unit sounds different).
        voice_pitch: f32,
    },
    /// End the current radio line (squelch tail). `interrupted` = cut off by a more urgent line.
    RadioEnd {
        /// Line id from [`AudioCommand::RadioSay`].
        line: u32,
        /// Chopped by a higher-priority line.
        interrupted: bool,
    },
}

/// Where the listener (camera) is. Supplied by the caller every frame.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Listener {
    /// Position in world space, m.
    pub pos: [f32; 3],
    /// Velocity in world space, m/s (for Doppler).
    pub vel: [f32; 3],
    /// Unit forward vector (world).
    pub forward: [f32; 3],
    /// Unit up vector (world).
    pub up: [f32; 3],
}

impl Default for Listener {
    fn default() -> Self {
        Self {
            pos: [0.0; 3],
            vel: [0.0; 3],
            forward: [0.0, 1.0, 0.0],
            up: [0.0, 0.0, 1.0],
        }
    }
}
