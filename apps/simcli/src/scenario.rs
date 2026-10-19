//! Scenario file format (`.ron`). The full reference with examples is `apps/simcli/SCENARIOS.md`.

use serde::{Deserialize, Serialize};
use wp_contracts::{AssistProfile, DamageMode, VehicleInput, PHYSICS_DT};

use crate::RunError;

/// A headless scenario: world setup, entities, scripted drivers and expectations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scenario {
    /// Scenario name; also the output folder (`out/<name>/`) and golden folder name.
    pub name: String,
    /// Master RNG seed.
    pub seed: u64,
    /// City to load.
    #[serde(default)]
    pub city: CitySpec,
    /// Start time of day, hours (0..24).
    #[serde(default = "default_hour")]
    pub time_of_day_h: f32,
    /// In-game seconds per simulated second.
    #[serde(default = "default_time_ratio")]
    pub time_ratio: f32,
    /// Damage mode (selects `data/modes/{arcade,sim}.toml`).
    #[serde(default)]
    pub mode: DamageMode,
    /// Default assist profile for entities that do not set their own.
    #[serde(default)]
    pub assists: AssistSpec,
    /// Civilian traffic density override (0..=1); `None` = city default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub traffic_density: Option<f32>,
    /// Police presence override (0..=1); `None` = city/time-of-day default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub police_presence: Option<f32>,
    /// Systems to step (`"vehicle,damage,world,police,net"`); `None` = all. `--systems` overrides.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub systems: Option<String>,
    /// Static rigid boxes (crash barriers, walls), added before any vehicle spawns.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub barriers: Vec<BarrierSpec>,
    /// Vehicles to spawn, in spawn order (entity ids are allocated in this order).
    pub entities: Vec<EntitySpec>,
    /// Simulated duration, s.
    pub duration_s: f32,
    /// `metrics.csv` sample rate, Hz (rounded to a whole number of physics ticks).
    #[serde(default = "default_sample_hz")]
    pub sample_hz: f32,
    /// Write a state hash every this many physics ticks (plus the final tick).
    #[serde(default = "default_hash_every")]
    pub hash_every: u64,
    /// Expectations; any failure makes the run fail.
    #[serde(default)]
    pub expect: Vec<Expect>,
    /// Expectations evaluated and reported but never failing: physics targets for systems that
    /// are still stubs. Owners move them into `expect` when their system lands.
    #[serde(default)]
    pub expect_pending: Vec<Expect>,
}

fn default_hour() -> f32 {
    12.0
}
fn default_time_ratio() -> f32 {
    10.0
}
fn default_sample_hz() -> f32 {
    60.0
}
fn default_hash_every() -> u64 {
    240
}

/// Which city to load.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CitySpec {
    /// Infinite flat asphalt (the `wp_world` stub).
    #[default]
    Flat,
    /// Procedurally generated city.
    Procedural {
        /// Generator seed (independent of the scenario seed).
        seed: u64,
        /// Blocks along X.
        blocks_x: u32,
        /// Blocks along Y.
        blocks_y: u32,
        /// Block edge length, m.
        block_m: f32,
    },
    /// City file relative to the data root (e.g. `"city/downtown.json"`).
    File(String),
}

/// Assist profile selection.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssistSpec {
    /// [`AssistProfile::keyboard_default`].
    #[default]
    Keyboard,
    /// [`AssistProfile::gamepad_default`].
    Gamepad,
    /// [`AssistProfile::wheel_default`].
    Wheel,
    /// [`AssistProfile::ai`] (no input filtering; good for scripted physics tests).
    Ai,
    /// Everything off: no ABS/TCS/ESC, raw inputs, manual gears, auto clutch.
    Off,
    /// Explicit profile.
    Custom(AssistProfile),
}

impl AssistSpec {
    /// The concrete profile.
    pub fn profile(&self) -> AssistProfile {
        match self {
            AssistSpec::Keyboard => AssistProfile::keyboard_default(),
            AssistSpec::Gamepad => AssistProfile::gamepad_default(),
            AssistSpec::Wheel => AssistProfile::wheel_default(),
            AssistSpec::Ai => AssistProfile::ai(),
            AssistSpec::Off => AssistProfile {
                abs: false,
                tcs: false,
                esc: false,
                steer_speed_sensitivity: 0.0,
                countersteer_help: 0.0,
                throttle_filter: 0.0,
                brake_filter: 0.0,
                auto_gears: false,
                auto_clutch: true,
            },
            AssistSpec::Custom(p) => *p,
        }
    }
}

/// A static box collider (`wp_sim::Sim::add_static_box`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BarrierSpec {
    /// Centre, world m.
    pub pos: [f32; 3],
    /// Full size `[x, y, z]`, m.
    pub size: [f32; 3],
    /// Heading about +Z, rad.
    #[serde(default)]
    pub yaw: f32,
}

/// One vehicle to spawn.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntitySpec {
    /// Scenario-local name used by expectations (`entity: "player"`).
    pub id: String,
    /// Car id (`data/cars/<car>.json`).
    pub car: String,
    /// Spawn position, world m.
    #[serde(default)]
    pub pos: [f32; 3],
    /// Spawn heading, rad about +Z (0 = facing +Y).
    #[serde(default)]
    pub yaw: f32,
    /// Assists for this entity (default: the scenario's).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assists: Option<AssistSpec>,
    /// Who drives. Input keyframes always apply on top of `scripted`; other drivers ignore them.
    #[serde(default)]
    pub driver: Driver,
    /// Input keyframes, ascending by `t_s`. Each holds until the next one.
    #[serde(default)]
    pub inputs: Vec<InputKey>,
}

/// Driver controlling an entity.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Driver {
    /// Input keyframes only (default).
    #[default]
    Scripted,
    /// Harness-side speed controller: holds `target_mps` with throttle/brake from `t_s` on
    /// (steer and the other fields come from the keyframes). Deterministic: reads only
    /// `VehicleState::speed_mps`. Used to arrive at crash tests at an exact speed.
    Cruise {
        /// Target forward speed, m/s.
        target_mps: f32,
        /// Controller start time, s (keyframes rule before it).
        #[serde(default)]
        from_s: f32,
        /// Controller end time, s (keyframes rule after it); `None` = until the end.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        until_s: Option<f32>,
    },
    /// Follow a road-network route at a target speed. Needs `wp_world` routes (prompt 05).
    FollowRoute {
        /// Route id in the city.
        route_id: String,
        /// Target speed, m/s.
        target_speed: f32,
    },
    /// Chase/attack another entity with the AI driver. Needs the AI driver (prompts 05/06).
    AiDriver {
        /// Scenario-local entity id to target.
        target: String,
    },
}

/// One input keyframe.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputKey {
    /// Time the keyframe starts, s.
    pub t_s: f32,
    /// Input held from `t_s`. `gear_up`/`gear_down` are edges: they apply to the first tick only.
    pub input: VehicleInput,
}

/// An expectation checked against the run. Ranges are inclusive.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Expect {
    /// Any per-entity summary metric (see `SCENARIOS.md` for the list) in `[min, max]`.
    Metric {
        /// Metric name.
        name: String,
        /// Entity (default: the first entity).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        entity: Option<String>,
        /// Inclusive minimum.
        min: f32,
        /// Inclusive maximum.
        max: f32,
    },
    /// Forward speed at time `t_s` in `[min, max]` m/s.
    SpeedAt {
        /// Entity (default: the first entity).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        entity: Option<String>,
        /// Sample time, s.
        t_s: f32,
        /// Inclusive minimum, m/s.
        min: f32,
        /// Inclusive maximum, m/s.
        max: f32,
    },
    /// Maximum |lateral acceleration| over the run in `[min, max]` g.
    MaxLateralG {
        /// Entity (default: the first entity).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        entity: Option<String>,
        /// Inclusive minimum, g.
        min: f32,
        /// Inclusive maximum, g.
        max: f32,
    },
    /// Maximum lattice node displacement from rest in `[min, max]` m.
    CrushDepth {
        /// Entity (default: the first entity).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        entity: Option<String>,
        /// Inclusive minimum, m.
        min: f32,
        /// Inclusive maximum, m.
        max: f32,
    },
    /// Number of distinct components failed (`ComponentFailed` events) in `[min, max]`.
    ComponentsFailed {
        /// Entity (default: the first entity).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        entity: Option<String>,
        /// Inclusive minimum.
        min: u32,
        /// Inclusive maximum.
        max: u32,
    },
    /// Whether the entity ends the run as a total loss.
    TotalLoss {
        /// Entity (default: the first entity).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        entity: Option<String>,
        /// Expected value.
        expected: bool,
    },
    /// A police pursuit started (`PoliceEvent` with kind [`crate::PURSUIT_STARTED_KIND`])
    /// before `t_s`.
    PursuitStartedBefore {
        /// Deadline, s.
        t_s: f32,
    },
    /// A `PoliceEvent` of `kind` was emitted before `t_s`.
    PoliceEventBefore {
        /// `wp_police` payload kind.
        kind: u16,
        /// Deadline, s.
        t_s: f32,
    },
    /// Number of events of a `SimEvent` variant (e.g. `"PartDetached"`), optionally only those
    /// naming `entity`, in `[min, max]`.
    EventCount {
        /// `SimEvent` variant name.
        event: String,
        /// Entity filter (default: all events of that kind).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        entity: Option<String>,
        /// Inclusive minimum.
        min: u32,
        /// Inclusive maximum.
        max: u32,
    },
}

impl Scenario {
    /// Parse scenario RON text (`implicit_some` is on, so `entity: "a"` works for options).
    pub fn from_ron(name: &str, text: &str) -> Result<Self, RunError> {
        let opts = ron::Options::default()
            .with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME);
        let sc: Scenario = opts
            .from_str(text)
            .map_err(|e| RunError::Scenario(name.to_string(), e.to_string()))?;
        sc.validate()
            .map_err(|e| RunError::Scenario(name.to_string(), e))?;
        Ok(sc)
    }

    /// Serialize to pretty RON (used for fuzz repro files and replay headers).
    pub fn to_ron(&self) -> String {
        let cfg = ron::ser::PrettyConfig::new()
            .struct_names(false)
            .extensions(ron::extensions::Extensions::IMPLICIT_SOME);
        ron::Options::default()
            .with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME)
            .to_string_pretty(self, cfg)
            .expect("scenario serializes")
    }

    /// Check internal consistency (positive duration, unique entity ids, sorted keyframes,
    /// expectations naming existing entities).
    pub fn validate(&self) -> Result<(), String> {
        if !(self.duration_s.is_finite() && self.duration_s > 0.0) {
            return Err(format!("duration_s = {} must be > 0", self.duration_s));
        }
        if self.entities.is_empty() {
            return Err("at least one entity is required".into());
        }
        for b in &self.barriers {
            if !b.size.iter().all(|x| x.is_finite() && *x > 0.0) {
                return Err(format!("barrier size {:?} must be positive", b.size));
            }
        }
        if self.hash_every == 0 {
            return Err("hash_every must be >= 1".into());
        }
        if !(self.sample_hz > 0.0 && self.sample_hz <= 240.0) {
            return Err(format!(
                "sample_hz = {} must be in (0, 240]",
                self.sample_hz
            ));
        }
        for (i, e) in self.entities.iter().enumerate() {
            if self.entities[..i].iter().any(|o| o.id == e.id) {
                return Err(format!("duplicate entity id {:?}", e.id));
            }
            if e.inputs.windows(2).any(|w| w[1].t_s < w[0].t_s) {
                return Err(format!("entity {:?}: input keyframes not ascending", e.id));
            }
            if let Driver::AiDriver { target } = &e.driver {
                self.entity_index(Some(target))?;
            }
        }
        for x in self.expect.iter().chain(&self.expect_pending) {
            self.entity_index(x.entity())?;
        }
        Ok(())
    }

    /// Index of a scenario entity by name (`None` = the first entity).
    pub fn entity_index(&self, name: Option<&String>) -> Result<usize, String> {
        match name {
            None => Ok(0),
            Some(n) => self
                .entities
                .iter()
                .position(|e| &e.id == n)
                .ok_or_else(|| format!("unknown entity {n:?}")),
        }
    }

    /// Total physics ticks the scenario runs.
    pub fn ticks(&self) -> u64 {
        (self.duration_s / PHYSICS_DT).round() as u64
    }

    /// Physics ticks between metric samples (at least 1).
    pub fn sample_every(&self) -> u64 {
        ((1.0 / self.sample_hz) / PHYSICS_DT).round().max(1.0) as u64
    }
}

impl Expect {
    /// The entity this expectation names, if any.
    pub fn entity(&self) -> Option<&String> {
        match self {
            Expect::Metric { entity, .. }
            | Expect::SpeedAt { entity, .. }
            | Expect::MaxLateralG { entity, .. }
            | Expect::CrushDepth { entity, .. }
            | Expect::ComponentsFailed { entity, .. }
            | Expect::TotalLoss { entity, .. }
            | Expect::EventCount { entity, .. } => entity.as_ref(),
            Expect::PursuitStartedBefore { .. } | Expect::PoliceEventBefore { .. } => None,
        }
    }
}

/// Compact single-line RON for any value, with `implicit_some` (used in reports).
pub fn to_ron_line<T: Serialize>(v: &T) -> String {
    ron::Options::default()
        .with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME)
        .to_string(v)
        .unwrap_or_else(|e| format!("<unserializable: {e}>"))
}

/// Convert seconds to the physics tick at which a keyframe/sample at `t_s` takes effect.
pub fn tick_of(t_s: f32) -> u64 {
    (t_s / PHYSICS_DT).round().max(0.0) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL: &str = r#"(
        name: "t",
        seed: 1,
        entities: [(id: "a", car: "debug_sedan", inputs: [(t_s: 0.0, input: (throttle: 1.0))])],
        duration_s: 1.0,
        expect: [speed_at(entity: "a", t_s: 0.5, min: 0.0, max: 100.0)],
    )"#;

    #[test]
    fn minimal_scenario_parses_with_defaults() {
        let sc = Scenario::from_ron("t", MINIMAL).unwrap();
        assert_eq!(sc.city, CitySpec::Flat);
        assert_eq!(sc.hash_every, 240);
        assert_eq!(sc.ticks(), 240);
        assert_eq!(sc.sample_every(), 4);
        assert_eq!(sc.entities[0].driver, Driver::Scripted);
    }

    #[test]
    fn ron_round_trip() {
        let sc = Scenario::from_ron("t", MINIMAL).unwrap();
        assert_eq!(Scenario::from_ron("t", &sc.to_ron()).unwrap(), sc);
    }

    #[test]
    fn rejects_unknown_entity_in_expectation() {
        let bad = MINIMAL.replace("entity: \"a\"", "entity: \"zz\"");
        assert!(Scenario::from_ron("t", &bad).is_err());
    }
}
