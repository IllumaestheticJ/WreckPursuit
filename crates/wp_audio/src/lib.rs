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
//! | tire squeal from combined slip × load, loose-surface scrub, surface rolling, thumps | [`tires`] |
//! | impacts by material × closing speed, glass, scrapes, debris | [`collision`] |
//! | siren cadences (wail, yelp, phaser, hi-lo), auto pattern, horn | [`siren`] |
//! | radio scanner queue (priority, tier delay, chop, music duck) | [`scanner`] |
//! | distance attenuation, Doppler, air absorption | [`spatial`] |
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
pub mod mixer;
pub mod scanner;
pub mod siren;
pub mod spatial;
pub mod tires;
pub mod types;

pub use data::{AudioConfig, AudioLibrary, CarAudioDef, EngineAudioDef, SamplePack};
pub use director::{AudioDirector, AudioFrame, DirectorStats, SirenOverride, VehicleAudioInput};
pub use scanner::{RadioLine, RadioPriority, ScannerTier};
pub use siren::{SirenMode, SirenPattern};
pub use types::{AudioCommand, Bus, Listener, Priority, VoiceId, VoiceParams};
