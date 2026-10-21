//! # wp_audio
//!
//! Audio driven entirely by simulation state and [`wp_contracts::SimEvent`]s, so it is free
//! content and always truthful. **Pure logic, no audio backend**: the [`AudioDirector`] turns
//! vehicle states, drained events and the listener pose into a deterministic stream of
//! [`AudioCommand`]s (play / set / stop named voices, bus ducks, radio lines). The browser
//! backend lives in `apps/testbed-web/src/audio/` (Web Audio); a native backend can consume
//! the same stream.
//!
//! | piece | module |
//! |---|---|
//! | engine layers (idle/low/mid/high/limiter × on/off throttle), turbo, supercharger, pops | [`engine`] |
//! | physically modelled engine voice: firing events, exhaust geometry, mods, per-frame state | [`engine_model`] |
//! | tire squeal from combined slip × load, loose-surface scrub, surface rolling, thumps | [`tires`] |
//! | impacts by material × closing speed, glass, scrapes, debris | [`collision`] |
//! | physically driven impact synthesis: panel modes, thump, crunch, glass tail, friction | [`impact`] |
//! | siren cadences (wail, yelp, phaser, hi-lo), auto pattern, horn | [`siren`] |
//! | siren v2: police-state cues, whoop, air horn, per-unit spread, siren limit | [`siren_unit`] |
//! | siren acoustics: occlusion, street slap echo, tunnel, interior muffle | [`siren_space`] |
//! | radio scanner queue (priority, tier delay, chop, music duck) | [`scanner`] |
//! | distance attenuation, Doppler, air absorption | [`spatial`] |
//! | audio state for body-less (kinematic) traffic and police | [`kinematic`] |
//! | voice culling (priority × audibility, global + per-bus limits) and diffing | [`mixer`] |
//! | config, per-car defs, sample-pack contract | [`data`] |
//!
//! Determinism: time comes only from ticks, maps are `BTreeMap`, randomness is a seeded
//! `wp_math::Rng`, trig goes through `wp_math::sin_cos`. Same inputs → identical commands.

#![warn(missing_docs)]

pub mod collision;
pub mod data;
pub mod director;
pub mod engine;
pub mod engine_model;
pub mod impact;
pub mod kinematic;
pub mod mixer;
pub mod scanner;
pub mod siren;
pub mod siren_space;
pub mod siren_unit;
pub mod spatial;
pub mod tires;
pub mod types;

pub use data::{AudioConfig, AudioLibrary, CarAudioDef, EngineAudioDef, MasterMix, SamplePack};
pub use director::{
    AudioDirector, AudioFrame, DirectorStats, DriverKind, EngineModelMode, SirenOverride,
    VehicleAudioInput, PHYSICAL_ENGINE_PACK,
};
pub use engine_model::{EngineFrame, EngineTune, EngineVoiceSpec};
pub use scanner::{RadioLine, RadioPriority, ScannerTier};
pub use siren::{SirenMode, SirenPattern};
pub use siren_space::{AcousticQuery, OpenAir, SirenSpaceFrame};
pub use siren_unit::SirenCue;
pub use types::{AudioCommand, Bus, Listener, Priority, VoiceId, VoiceParams};
