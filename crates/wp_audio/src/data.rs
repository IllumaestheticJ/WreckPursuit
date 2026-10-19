//! Audio data: the tuning config (`data/audio/audio_config.json`), per-car audio definitions
//! (`data/audio/cars/<car_id>.json`) and sample packs (`data/audio/packs/<id>.json`).
//!
//! The sample-pack format is the contract with sound designers; it is specified for humans in
//! `crates/wp_audio/README.md`. Every file's `id` must equal its file stem.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use wp_contracts::{VehicleClass, VehicleDef};

use crate::scanner::ScannerConfig;
use crate::siren::{SirenCadence, SirenMode, SirenPattern};
use crate::types::Bus;

/// Cue names the director requests. A pack of the matching [`PackKind`] must define all of
/// its group (checked by [`AudioLibrary::validate`]).
pub mod cues {
    /// Required in every `Engine` pack (besides its `engine_layers`).
    pub const ENGINE_EXTRA: [&str; 5] = [
        LIMITER,
        TURBO_WHISTLE,
        TURBO_BLOWOFF,
        SUPERCHARGER_WHINE,
        EXHAUST_POP,
    ];
    /// Rev limiter bounce loop.
    pub const LIMITER: &str = "engine/limiter";
    /// Turbo whistle loop (pitch follows boost).
    pub const TURBO_WHISTLE: &str = "turbo/whistle";
    /// Blow-off valve one-shot on throttle lift under boost.
    pub const TURBO_BLOWOFF: &str = "turbo/blowoff";
    /// Supercharger whine loop (pitch follows rpm).
    pub const SUPERCHARGER_WHINE: &str = "supercharger/whine";
    /// Overrun exhaust pop one-shot.
    pub const EXHAUST_POP: &str = "exhaust/pop";

    /// Tire squeal loop on paved surfaces.
    pub const SQUEAL: &str = "tire/squeal";
    /// Loose-surface scrub loop.
    pub const SCRUB: &str = "tire/scrub_loose";
    /// Surface rolling loops, indexed like `wp_contracts::SurfaceType` declaration order.
    pub const SURFACES: [&str; 8] = [
        "surface/asphalt",
        "surface/wet_asphalt",
        "surface/concrete",
        "surface/gravel",
        "surface/grass",
        "surface/dirt",
        "surface/curb",
        "surface/metal",
    ];
    /// Suspension bump one-shot.
    pub const THUMP: &str = "suspension/thump";
    /// Landing after airtime one-shot.
    pub const LAND: &str = "suspension/land";
    /// Low-speed soft contact one-shot.
    pub const THUD: &str = "impact/thud";
    /// Plastic bumper crack one-shot.
    pub const PLASTIC_CRACK: &str = "impact/plastic_crack";
    /// Steel panel dent one-shot.
    pub const PANEL_DENT: &str = "impact/panel_dent";
    /// Structural crunch one-shot (rails, pillars).
    pub const STRUCTURAL_CRUNCH: &str = "impact/structural_crunch";
    /// Glass shatter one-shot.
    pub const GLASS_SHATTER: &str = "impact/glass_shatter";
    /// Cast-metal clank one-shot (hubs, knuckles).
    pub const CAST_CLANK: &str = "impact/cast_clank";
    /// Metal scrape loop (sustained contact).
    pub const SCRAPE_METAL: &str = "scrape/metal";
    /// Plastic scrape loop.
    pub const SCRAPE_PLASTIC: &str = "scrape/plastic";
    /// Part tearing off one-shot.
    pub const PART_TEAR: &str = "debris/part_tear";
    /// Wheel tearing off one-shot.
    pub const WHEEL_OFF: &str = "debris/wheel_off";
    /// Engine failure bang one-shot.
    pub const ENGINE_FAIL: &str = "mech/engine_fail";
    /// Cooling damage hiss loop.
    pub const STEAM_HISS: &str = "mech/steam_hiss";
    /// Passenger-car horn loop.
    pub const HORN_STANDARD: &str = "horn/standard";
    /// Truck / SUV horn loop.
    pub const HORN_TRUCK: &str = "horn/truck";

    /// Every cue a `World` pack must define.
    pub fn world() -> Vec<&'static str> {
        let mut v = vec![
            SQUEAL,
            SCRUB,
            THUMP,
            LAND,
            THUD,
            PLASTIC_CRACK,
            PANEL_DENT,
            STRUCTURAL_CRUNCH,
            GLASS_SHATTER,
            CAST_CLANK,
            SCRAPE_METAL,
            SCRAPE_PLASTIC,
            PART_TEAR,
            WHEEL_OFF,
            ENGINE_FAIL,
            STEAM_HISS,
            HORN_STANDARD,
            HORN_TRUCK,
        ];
        v.extend_from_slice(&SURFACES);
        v
    }

    /// Every cue a `Siren` pack must define (one loop per pattern).
    pub const SIREN: [&str; 4] = ["siren/wail", "siren/yelp", "siren/phaser", "siren/hilo"];

    /// Every cue a `Radio` pack must define (used by the backend around `RadioSay`).
    pub const RADIO: [&str; 3] = ["radio/squelch_on", "radio/squelch_off", "radio/static"];
}

/// Synth names the procedural backend implements (`apps/testbed-web/src/audio/synth.ts`).
pub const KNOWN_SYNTHS: [&str; 15] = [
    "engine", "noise", "tone", "siren", "horn", "impact", "glass", "thump", "squeal", "whine",
    "blowoff", "pop", "squelch", "clatter", "hiss",
];

// ----- config ------------------------------------------------------------------------------

/// Distance attenuation, Doppler and air absorption.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpatialConfig {
    /// Distance at which gain is 1, m.
    pub ref_distance_m: f32,
    /// Beyond this a voice is silent (culled), m.
    pub max_distance_m: f32,
    /// Inverse-distance rolloff factor (Web Audio "inverse" model).
    pub rolloff: f32,
    /// Speed of sound, m/s.
    pub speed_of_sound_mps: f32,
    /// Doppler strength, 0 (off) ..= 1 (physical).
    pub doppler_factor: f32,
    /// Doppler ratio clamp `[1/max, max]`.
    pub max_doppler: f32,
    /// Low-pass cutoff at 0 m, Hz.
    pub lowpass_near_hz: f32,
    /// Distance at which the cutoff has halved, m.
    pub lowpass_half_distance_m: f32,
}

/// Engine mix tuning.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EngineMix {
    /// Overall engine gain.
    pub gain: f32,
    /// Throttle → load smoothing time constant, s.
    pub load_tau_s: f32,
    /// Load floor: even off throttle the on-layers keep this share (engine braking noise).
    pub min_load: f32,
    /// Limiter layer fades in over this many rpm below `rev_limit_rpm`.
    pub limiter_window_rpm: f32,
    /// Turbo spool-up time constant, s.
    pub turbo_spool_tau_s: f32,
    /// Turbo spool-down time constant, s.
    pub turbo_fall_tau_s: f32,
    /// Turbo starts making boost at this fraction of redline.
    pub turbo_onset_frac: f32,
    /// Turbo reaches full boost at this fraction of redline.
    pub turbo_full_frac: f32,
    /// Minimum boost (0..1) for a blow-off on throttle lift.
    pub blowoff_min_boost: f32,
    /// Overrun pops per second (expected) at full overrun.
    pub pop_rate_per_s: f32,
    /// Pops only above this fraction of redline.
    pub pop_min_redline_frac: f32,
    /// Turbo / supercharger loop gain.
    pub forced_induction_gain: f32,
}

/// Tire, surface and suspension tuning.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TireMix {
    /// Slip ratio treated as "peak" (1.0 in combined slip).
    pub slip_ratio_peak: f32,
    /// Slip angle treated as "peak", rad.
    pub slip_angle_peak_rad: f32,
    /// Combined slip (1 = peak) where squeal starts.
    pub squeal_threshold: f32,
    /// Combined slip range over which squeal reaches full level.
    pub squeal_range: f32,
    /// Squeal gain.
    pub squeal_gain: f32,
    /// Squeal multiplier on wet asphalt.
    pub wet_squeal_scale: f32,
    /// Loose-surface scrub gain.
    pub scrub_gain: f32,
    /// Speed at which rolling noise reaches full level, m/s.
    pub rumble_speed_ref_mps: f32,
    /// Rolling-noise gain.
    pub rumble_gain: f32,
    /// Weight of `FfbOutput::road_rumble` added to rolling noise.
    pub ffb_rumble_weight: f32,
    /// Suspension compression rate that triggers a thump, m/s.
    pub thump_rate_mps: f32,
    /// Compression rate for a full-volume thump, m/s.
    pub thump_rate_ref_mps: f32,
    /// Airtime before touchdown counts as a landing, s.
    pub land_airtime_s: f32,
    /// Minimum time between thumps on one wheel, s.
    pub thump_debounce_s: f32,
}

/// Collision tuning. Speeds are the contact's closing speed (`rel_speed`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImpactMix {
    /// Below this closing speed no impact one-shot plays, m/s.
    pub min_speed_mps: f32,
    /// Below this the impact is a soft thud regardless of material, m/s.
    pub thud_max_speed_mps: f32,
    /// Panels add a structural crunch layer above this, m/s.
    pub panel_crunch_speed_mps: f32,
    /// Structural members crunch (instead of dent) above this, m/s.
    pub structural_crunch_speed_mps: f32,
    /// Glass shatters above this, m/s (once per glass region).
    pub glass_break_speed_mps: f32,
    /// Normal impulse giving a full-volume impact, N·s.
    pub impulse_ref_ns: f32,
    /// Minimum time between impacts of the same pair, s.
    pub debounce_s: f32,
    /// A new impact inside the debounce window still plays if this much faster.
    pub louder_factor: f32,
    /// Sustained contact scrapes above this tangential speed, m/s.
    pub scrape_min_speed_mps: f32,
    /// Tangential speed for a full-volume scrape, m/s.
    pub scrape_speed_ref_mps: f32,
    /// A scrape fades out this long after the last contact, s.
    pub scrape_release_s: f32,
    /// Impact gain.
    pub impact_gain: f32,
    /// Scrape gain.
    pub scrape_gain: f32,
}

/// Siren behaviour.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SirenConfig {
    /// Cadence per pattern.
    pub cadence: BTreeMap<SirenPattern, SirenCadence>,
    /// `Auto` mode switches to wail above this speed, m/s.
    pub auto_wail_above_mps: f32,
    /// `Auto` mode switches to yelp below this speed, m/s.
    pub auto_yelp_below_mps: f32,
    /// Siren gain.
    pub gain: f32,
    /// Horn gain.
    pub horn_gain: f32,
}

/// Global audio tuning (`data/audio/audio_config.json`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AudioConfig {
    /// Must be `"audio_config"`.
    pub id: String,
    /// Maximum simultaneous voices.
    pub max_voices: usize,
    /// Per-bus voice limits (buses absent here are limited only by `max_voices`).
    pub bus_max_voices: BTreeMap<Bus, usize>,
    /// Culling weight per [`crate::Priority`] (Low, Normal, High, Critical).
    pub priority_weights: [f32; 4],
    /// Score multiplier for voices that are already playing (prevents flapping).
    pub keep_bonus: f32,
    /// Voices quieter than this (after attenuation) are not played.
    pub min_audible_gain: f32,
    /// Fade time for stops and culls, s.
    pub stop_fade_s: f32,
    /// Spatial model.
    pub spatial: SpatialConfig,
    /// Engine mix.
    pub engine: EngineMix,
    /// Tire / surface / suspension mix.
    pub tires: TireMix,
    /// Collision mix.
    pub impacts: ImpactMix,
    /// Sirens and horns.
    pub sirens: SirenConfig,
    /// Radio scanner.
    pub scanner: ScannerConfig,
}

// ----- cars --------------------------------------------------------------------------------

/// Cylinder arrangement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum EngineLayout {
    /// Inline / straight.
    Inline,
    /// V engine.
    V,
    /// Flat / boxer.
    Flat,
    /// Rotary (Wankel); `cylinders` = rotors.
    Rotary,
}

/// Firing-order feel: how even the exhaust pulses are.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum FiringFeel {
    /// Evenly spaced pulses (inline engines, flat-plane V8s).
    Even,
    /// Cross-plane V8 burble (uneven per bank, strong half-order).
    CrossPlane,
    /// Odd-fire V6 / V10 lope.
    OddFire,
    /// Unequal-length boxer rumble.
    Rumble,
}

/// Per-car engine sound description. Drives the procedural synth and layer tuning.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EngineAudioDef {
    /// Number of cylinders (rotors for rotary).
    pub cylinders: u8,
    /// Cylinder layout.
    pub layout: EngineLayout,
    /// Firing-order feel.
    pub firing: FiringFeel,
    /// 0 = all exhaust, 1 = all induction (intake roar vs tailpipe note).
    pub induction_exhaust_balance: f32,
    /// Has a turbocharger (whistle + blow-off).
    pub turbo: bool,
    /// Has a supercharger (whine).
    pub supercharger: bool,
    /// Engine gain multiplier for this car.
    #[serde(default = "one")]
    pub gain: f32,
}

impl EngineAudioDef {
    /// Firing frequency (fundamental of the exhaust note) at `rpm` for a four-stroke engine,
    /// Hz: `rpm / 60 × cylinders / 2` (rotary: one pulse per rotor per output revolution).
    pub fn firing_hz(&self, rpm: f32) -> f32 {
        let per_rev = match self.layout {
            EngineLayout::Rotary => self.cylinders as f32,
            _ => self.cylinders as f32 * 0.5,
        };
        rpm.max(0.0) / 60.0 * per_rev
    }

    /// Synth parameters the procedural engine voice needs from the car.
    pub fn synth_params(&self) -> BTreeMap<String, f32> {
        let uneven = match self.firing {
            FiringFeel::Even => 0.0,
            FiringFeel::CrossPlane => 0.8,
            FiringFeel::OddFire => 0.5,
            FiringFeel::Rumble => 0.6,
        };
        let mut m = BTreeMap::new();
        m.insert("cylinders".to_string(), self.cylinders as f32);
        m.insert("uneven".to_string(), uneven);
        m.insert("balance".to_string(), self.induction_exhaust_balance);
        m
    }
}

/// Siren fitment of a police car.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SirenDef {
    /// Siren sample pack id.
    pub pack: String,
    /// Mode used when the siren switches on.
    pub default_mode: SirenMode,
}

/// Per-car audio (`data/audio/cars/<car_id>.json`, `id` = the car id in `data/cars`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CarAudioDef {
    /// Car id; must equal the file stem and a `data/cars/<id>.json`.
    pub id: String,
    /// Engine description.
    pub engine: EngineAudioDef,
    /// Engine sample pack id.
    pub engine_pack: String,
    /// World sample pack id (tires, impacts, horn).
    #[serde(default = "default_world_pack")]
    pub world_pack: String,
    /// Horn cue in the world pack.
    #[serde(default = "default_horn")]
    pub horn_cue: String,
    /// Siren, if fitted.
    #[serde(default)]
    pub siren: Option<SirenDef>,
}

fn one() -> f32 {
    1.0
}

fn default_world_pack() -> String {
    "world_proc".to_string()
}

fn default_horn() -> String {
    cues::HORN_STANDARD.to_string()
}

impl CarAudioDef {
    /// Fallback for a car with no `data/audio/cars` file: a generic NA four (V8 for muscle and
    /// trucks), siren on police classes. Keeps every car audible.
    pub fn fallback(def: &VehicleDef) -> Self {
        let big = matches!(
            def.class,
            VehicleClass::Muscle | VehicleClass::Truck | VehicleClass::PoliceSuv
        );
        let police = matches!(
            def.class,
            VehicleClass::PoliceCruiser | VehicleClass::PoliceInterceptor | VehicleClass::PoliceSuv
        );
        Self {
            id: def.id.clone(),
            engine: EngineAudioDef {
                cylinders: if big { 8 } else { 4 },
                layout: if big {
                    EngineLayout::V
                } else {
                    EngineLayout::Inline
                },
                firing: if big {
                    FiringFeel::CrossPlane
                } else {
                    FiringFeel::Even
                },
                induction_exhaust_balance: 0.4,
                turbo: false,
                supercharger: false,
                gain: 1.0,
            },
            engine_pack: if big {
                "engine_proc_v8"
            } else {
                "engine_proc_i4"
            }
            .to_string(),
            world_pack: default_world_pack(),
            horn_cue: if big {
                cues::HORN_TRUCK
            } else {
                cues::HORN_STANDARD
            }
            .to_string(),
            siren: police.then(|| SirenDef {
                pack: "siren_proc".to_string(),
                default_mode: SirenMode::Auto,
            }),
        }
    }
}

// ----- packs -------------------------------------------------------------------------------

/// What a pack provides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum PackKind {
    /// Engine layers + forced induction + pops.
    Engine,
    /// Tires, surfaces, suspension, impacts, scrapes, debris, horns.
    World,
    /// Siren pattern loops.
    Siren,
    /// Scanner squelch and static.
    Radio,
}

/// The rpm a layer was recorded / tuned at.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum RpmAnchor {
    /// The car's `idle_rpm`.
    Idle,
    /// An absolute rpm (recorded samples).
    Rpm(f32),
    /// A fraction of the car's `redline_rpm` (procedural packs shared by many cars).
    RedlineFrac(f32),
}

/// Which throttle state a layer belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ThrottleSide {
    /// On throttle (load).
    On,
    /// Off throttle (overrun / coast).
    Off,
    /// Used in both sets (typically idle).
    Both,
}

/// One engine layer of a granular engine pack.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EngineLayerDef {
    /// Cue name in this pack.
    pub cue: String,
    /// Root rpm of the layer.
    pub anchor: RpmAnchor,
    /// Throttle set.
    pub throttle: ThrottleSide,
}

/// Where a cue's audio comes from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CueSource {
    /// Recorded files (relative to `data/audio/packs/<pack>/`); one is picked per play.
    Samples {
        /// File names (`.ogg` / `.wav`).
        files: Vec<String>,
    },
    /// Procedural placeholder rendered by the backend.
    Synth {
        /// Synth name (one of [`KNOWN_SYNTHS`]).
        synth: String,
        /// Synth parameters (meaning per synth, see the README).
        #[serde(default)]
        params: BTreeMap<String, f32>,
    },
}

/// One cue of a pack.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CueDef {
    /// Audio source.
    pub source: CueSource,
    /// Linear gain of this cue.
    #[serde(default = "one")]
    pub gain: f32,
    /// Seamless loop.
    #[serde(default)]
    pub looped: bool,
    /// Length of a one-shot for voice accounting, s (required when `looped` is false).
    #[serde(default)]
    pub duration_s: Option<f32>,
    /// Speed the loop was recorded at (surface loops), m/s; pitch = speed / root.
    #[serde(default)]
    pub root_speed_mps: Option<f32>,
}

/// A sample pack (`data/audio/packs/<id>.json`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SamplePack {
    /// Pack id; must equal the file stem.
    pub id: String,
    /// What the pack provides.
    pub kind: PackKind,
    /// Free text for humans.
    #[serde(default)]
    pub description: String,
    /// Engine layers (Engine packs only), any order.
    #[serde(default)]
    pub engine_layers: Vec<EngineLayerDef>,
    /// Cues by name.
    pub cues: BTreeMap<String, CueDef>,
}

// ----- library -----------------------------------------------------------------------------

/// Error loading audio data.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioDataError(pub String);

impl std::fmt::Display for AudioDataError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for AudioDataError {}

/// Everything the director needs: config, per-car defs and packs.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioLibrary {
    /// Global tuning.
    pub config: AudioConfig,
    /// Per-car audio by car id.
    pub cars: BTreeMap<String, CarAudioDef>,
    /// Packs by id.
    pub packs: BTreeMap<String, SamplePack>,
}

mod embedded {
    include!(concat!(env!("OUT_DIR"), "/embedded_audio.rs"));
}

fn parse<T: serde::de::DeserializeOwned>(
    kind: &str,
    id: &str,
    text: &str,
) -> Result<T, AudioDataError> {
    serde_json::from_str(text).map_err(|e| AudioDataError(format!("{kind}/{id}: {e}")))
}

impl AudioLibrary {
    /// Parse a library from `(kind, id, text)` files (`kind` = `config` | `cars` | `packs`).
    /// Checks `id` == file stem; run [`AudioLibrary::validate`] for cross references.
    pub fn from_files<'a>(
        files: impl IntoIterator<Item = (&'a str, &'a str, &'a str)>,
    ) -> Result<Self, AudioDataError> {
        let mut config = None;
        let mut cars = BTreeMap::new();
        let mut packs = BTreeMap::new();
        for (kind, id, text) in files {
            match kind {
                "config" => {
                    let c: AudioConfig = parse(kind, id, text)?;
                    check_id(kind, id, &c.id)?;
                    config = Some(c);
                }
                "cars" => {
                    let c: CarAudioDef = parse(kind, id, text)?;
                    check_id(kind, id, &c.id)?;
                    cars.insert(c.id.clone(), c);
                }
                "packs" => {
                    let p: SamplePack = parse(kind, id, text)?;
                    check_id(kind, id, &p.id)?;
                    packs.insert(p.id.clone(), p);
                }
                other => return Err(AudioDataError(format!("unknown audio data kind {other:?}"))),
            }
        }
        let config = config.ok_or_else(|| AudioDataError("missing audio_config.json".into()))?;
        Ok(Self {
            config,
            cars,
            packs,
        })
    }

    /// The library embedded at build time from `data/audio/` (works in the browser).
    pub fn embedded() -> Result<Self, AudioDataError> {
        Self::from_files(embedded::EMBEDDED.iter().copied())
    }

    /// Load from a `data/audio` directory on disk (native tools and tests).
    pub fn load_dir(dir: impl AsRef<Path>) -> Result<Self, AudioDataError> {
        let dir = dir.as_ref();
        let io = |e: std::io::Error| AudioDataError(format!("{}: {e}", dir.display()));
        let mut owned: Vec<(String, String, String)> = Vec::new();
        let cfg = std::fs::read_to_string(dir.join("audio_config.json")).map_err(io)?;
        owned.push(("config".into(), "audio_config".into(), cfg));
        for kind in ["cars", "packs"] {
            let mut paths: Vec<_> = std::fs::read_dir(dir.join(kind))
                .map_err(io)?
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("json"))
                .collect();
            paths.sort();
            for p in paths {
                let id = p
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                owned.push((kind.into(), id, std::fs::read_to_string(&p).map_err(io)?));
            }
        }
        Self::from_files(
            owned
                .iter()
                .map(|(k, i, t)| (k.as_str(), i.as_str(), t.as_str())),
        )
    }

    /// Audio def for a car: its file, or [`CarAudioDef::fallback`].
    pub fn car_or_fallback(&self, def: &VehicleDef) -> CarAudioDef {
        self.cars
            .get(&def.id)
            .cloned()
            .unwrap_or_else(|| CarAudioDef::fallback(def))
    }

    /// Cross-reference checks. Returns every problem found (empty = valid).
    pub fn validate(&self) -> Vec<String> {
        let mut errs = Vec::new();
        for car in self.cars.values() {
            match self.packs.get(&car.engine_pack) {
                Some(p) if p.kind == PackKind::Engine => {}
                Some(_) => errs.push(format!(
                    "cars/{}: engine_pack {} is not an Engine pack",
                    car.id, car.engine_pack
                )),
                None => errs.push(format!(
                    "cars/{}: unknown engine_pack {}",
                    car.id, car.engine_pack
                )),
            }
            match self.packs.get(&car.world_pack) {
                Some(p) if p.kind == PackKind::World => {
                    if !p.cues.contains_key(&car.horn_cue) {
                        errs.push(format!(
                            "cars/{}: horn_cue {} not in {}",
                            car.id, car.horn_cue, p.id
                        ));
                    }
                }
                _ => errs.push(format!(
                    "cars/{}: world_pack {} missing or not a World pack",
                    car.id, car.world_pack
                )),
            }
            if let Some(s) = &car.siren {
                if self.packs.get(&s.pack).map(|p| p.kind) != Some(PackKind::Siren) {
                    errs.push(format!(
                        "cars/{}: siren pack {} missing or not a Siren pack",
                        car.id, s.pack
                    ));
                }
            }
            let e = &car.engine;
            if e.cylinders == 0 || e.cylinders > 16 {
                errs.push(format!(
                    "cars/{}: cylinders {} out of range 1..=16",
                    car.id, e.cylinders
                ));
            }
            if !(0.0..=1.0).contains(&e.induction_exhaust_balance) {
                errs.push(format!(
                    "cars/{}: induction_exhaust_balance outside 0..=1",
                    car.id
                ));
            }
        }
        for p in self.packs.values() {
            let required: Vec<&str> = match p.kind {
                PackKind::Engine => cues::ENGINE_EXTRA.to_vec(),
                PackKind::World => cues::world(),
                PackKind::Siren => cues::SIREN.to_vec(),
                PackKind::Radio => cues::RADIO.to_vec(),
            };
            for c in required {
                if !p.cues.contains_key(c) {
                    errs.push(format!("packs/{}: missing required cue {c}", p.id));
                }
            }
            if p.kind == PackKind::Engine {
                let on = p
                    .engine_layers
                    .iter()
                    .filter(|l| l.throttle != ThrottleSide::Off)
                    .count();
                let off = p
                    .engine_layers
                    .iter()
                    .filter(|l| l.throttle != ThrottleSide::On)
                    .count();
                if on < 2 || off < 1 {
                    errs.push(format!(
                        "packs/{}: needs ≥ 2 on-throttle and ≥ 1 off-throttle layers",
                        p.id
                    ));
                }
                for l in &p.engine_layers {
                    match p.cues.get(&l.cue) {
                        Some(c) if c.looped => {}
                        Some(_) => errs.push(format!(
                            "packs/{}: engine layer {} must be looped",
                            p.id, l.cue
                        )),
                        None => errs.push(format!(
                            "packs/{}: engine layer cue {} not defined",
                            p.id, l.cue
                        )),
                    }
                }
            } else if !p.engine_layers.is_empty() {
                errs.push(format!(
                    "packs/{}: engine_layers only allowed in Engine packs",
                    p.id
                ));
            }
            for (name, c) in &p.cues {
                if !c.looped && c.duration_s.is_none() {
                    errs.push(format!(
                        "packs/{}: one-shot cue {name} needs duration_s",
                        p.id
                    ));
                }
                match &c.source {
                    CueSource::Synth { synth, .. } => {
                        if !KNOWN_SYNTHS.contains(&synth.as_str()) {
                            errs.push(format!("packs/{}: cue {name}: unknown synth {synth}", p.id));
                        }
                    }
                    CueSource::Samples { files } => {
                        if files.is_empty() {
                            errs.push(format!(
                                "packs/{}: cue {name}: Samples needs ≥ 1 file",
                                p.id
                            ));
                        }
                    }
                }
            }
        }
        for pattern in SirenPattern::ALL {
            if !self.config.sirens.cadence.contains_key(&pattern) {
                errs.push(format!("audio_config: sirens.cadence missing {pattern:?}"));
            }
        }
        errs
    }
}

fn check_id(kind: &str, stem: &str, id: &str) -> Result<(), AudioDataError> {
    if stem == id {
        Ok(())
    } else {
        Err(AudioDataError(format!(
            "{kind}/{stem}.json: id {id:?} must equal the file stem"
        )))
    }
}
