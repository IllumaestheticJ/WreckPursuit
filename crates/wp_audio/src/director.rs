//! [`AudioDirector`]: the one entry point. Each frame the caller passes vehicle states, the
//! events drained since the last frame and the listener; the director returns the
//! [`AudioCommand`]s for the backend. Same inputs → same commands, bit for bit.

use std::collections::{BTreeMap, BTreeSet};

use wp_contracts::{
    BodyRegion, Component, EntityId, LoadedCar, SimEvent, VehicleState, GRAVITY_MPS2, PHYSICS_DT,
};
use wp_math::{clamp01, Rng};
use wp_police::PoliceEvent;

use crate::collision::{impact_gain, impact_layers, pair_key, ImpactLayer, MaterialMap, PairState};
use crate::data::{cues, AudioLibrary, CarAudioDef};
use crate::engine::{
    engine_lod, engine_synth, update_engine, EngineAudioState, EngineInput, EngineLayers,
    EngineLimits,
};
use crate::engine_model::{EngineFrame, EngineTune, EngineVoiceSpec};
use crate::impact::{
    body_params, glass_params, reduced_mass, scrape_force, scrape_rate_hz, CrunchState, Hit,
};
use crate::mixer::{CueRef, CueTable, Desire, MixerStats, VoiceMixer};
use crate::scanner::{RadioLine, RadioPriority, Scanner, ScannerTier};
use crate::siren::{update_siren, SirenMode, SirenPattern, SirenState};
use crate::spatial::{dot, len, rotate, sub, world_to_local};
use crate::tires::{update_tires, TireAudioState};
use crate::types::{slot, AudioCommand, Bus, Listener, Priority, VoiceId};

/// Who drives a vehicle (mirrors `wp_sim::VehicleKind`; `wp_audio` does not depend on the sim).
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Default,
    serde::Serialize,
    serde::Deserialize,
)]
pub enum DriverKind {
    /// Human player.
    #[default]
    Player,
    /// AI opponent racer.
    AiRacer,
    /// AI police unit: always siren-capable (Auto mode unless the car says otherwise).
    Police,
    /// Physical traffic: engine at `engine.traffic_gain`, low culling priority.
    Traffic,
}

/// One vehicle's audio-relevant input for a frame.
#[derive(Debug, Clone, Copy)]
pub struct VehicleAudioInput<'a> {
    /// Entity.
    pub entity: EntityId,
    /// Car id (`VehicleDef::id`); must have been registered with [`AudioDirector::register_car`].
    pub car_id: &'a str,
    /// Latest state.
    pub state: &'a VehicleState,
    /// Throttle the vehicle received, 0..=1 (`VehicleInput::throttle` after assists if known).
    pub throttle: f32,
    /// Throttle actually reaching the engine (`wp_vehicle::Telemetry::throttle`: after the
    /// pedal filter, TCS and the gearbox's shift cut / blip), if the caller has it. The engine
    /// sound uses it for load so shift dips, TCS cuts and rev-match blips are heard; `None`
    /// falls back to `throttle`.
    pub throttle_applied: Option<f32>,
    /// Horn held (`VehicleInput::horn`).
    pub horn: bool,
    /// Siren requested (`VehicleInput::siren`).
    pub siren: bool,
    /// `FfbOutput::road_rumble` if available, else 0.
    pub road_rumble: f32,
    /// Who drives it.
    pub kind: DriverKind,
}

/// Everything the director needs for one frame.
#[derive(Debug, Clone, Copy)]
pub struct AudioFrame<'a> {
    /// Simulation tick of the newest state (time comes only from ticks).
    pub tick: u64,
    /// Camera / listener.
    pub listener: Listener,
    /// Entity the player drives / the camera follows (its voices win culling ties).
    pub focus: Option<EntityId>,
    /// Vehicles, any order (processed ascending by entity).
    pub vehicles: &'a [VehicleAudioInput<'a>],
    /// Events drained since the previous frame, in emission order.
    pub events: &'a [SimEvent],
}

/// Resolved per-car audio (shared by every entity of that car).
#[derive(Debug, Clone)]
struct CarAudio {
    def: CarAudioDef,
    limits: EngineLimits,
    layers: EngineLayers,
    layer_cues: Vec<CueRef>,
    limiter: CueRef,
    turbo: CueRef,
    blowoff: CueRef,
    supercharger: CueRef,
    pop: CueRef,
    horn: CueRef,
    /// Police air horn (`horn/police`), used for police units whose car has a standard horn.
    horn_police: CueRef,
    siren: [CueRef; 4],
    world: WorldCues,
    materials: MaterialMap,
    static_load_n: f32,
    /// Vehicle mass, kg (impact energy, panel size, thump pitch).
    mass_kg: f32,
    /// Chassis-space box of the cabin (interior-camera detection), if the lattice has one.
    cabin: Option<([f32; 3], [f32; 3])>,
    /// Loaded wheel radii FL, FR, RL, RR, m (wheelspin slip speed).
    wheel_radius_m: [f32; 4],
    /// Gameplay class (exhaust geometry of the physical engine model).
    class: wp_contracts::VehicleClass,
    /// Final drive ratio (differential whine).
    final_drive: f32,
    /// Physical engine synth params of the stock car.
    physical_stock: BTreeMap<String, f32>,
}

/// Cues of one world pack.
#[derive(Debug, Clone)]
struct WorldCues {
    squeal: CueRef,
    scrub: CueRef,
    spin: CueRef,
    spin_loose: CueRef,
    surfaces: [CueRef; 8],
    thump: CueRef,
    land: CueRef,
    impacts: BTreeMap<ImpactLayer, CueRef>,
    scrape_metal: CueRef,
    scrape_plastic: CueRef,
    part_tear: CueRef,
    wheel_off: CueRef,
    engine_fail: CueRef,
    steam: CueRef,
    tire_burst: CueRef,
    rattle: CueRef,
    /// Optional gear-shift clunk (`drivetrain/shift`); silent when the pack lacks it.
    shift: CueRef,
    /// Optional synthesised panel impact (`impact/body`, owner row 74).
    body: CueRef,
    /// Optional granular crunch loop (`impact/crunch`).
    crunch: CueRef,
}

impl WorldCues {
    fn new(t: &mut CueTable, pack: &str) -> Self {
        let mut impacts = BTreeMap::new();
        for l in [
            ImpactLayer::Thud,
            ImpactLayer::PlasticCrack,
            ImpactLayer::PanelDent,
            ImpactLayer::StructuralCrunch,
            ImpactLayer::GlassShatter,
            ImpactLayer::CastClank,
        ] {
            impacts.insert(l, t.get(pack, l.cue(), false));
        }
        Self {
            squeal: t.get(pack, cues::SQUEAL, true),
            scrub: t.get(pack, cues::SCRUB, true),
            spin: t.get(pack, cues::SPIN, true),
            spin_loose: t.get(pack, cues::SPIN_LOOSE, true),
            surfaces: cues::SURFACES.map(|c| t.get(pack, c, true)),
            thump: t.get(pack, cues::THUMP, false),
            land: t.get(pack, cues::LAND, false),
            impacts,
            scrape_metal: t.get(pack, cues::SCRAPE_METAL, true),
            scrape_plastic: t.get(pack, cues::SCRAPE_PLASTIC, true),
            part_tear: t.get(pack, cues::PART_TEAR, false),
            wheel_off: t.get(pack, cues::WHEEL_OFF, false),
            engine_fail: t.get(pack, cues::ENGINE_FAIL, false),
            steam: t.get(pack, cues::STEAM_HISS, true),
            tire_burst: t.get(pack, cues::TIRE_BURST, false),
            rattle: t.get(pack, cues::BODY_RATTLE, true),
            shift: t.get(pack, cues::SHIFT, false),
            body: t.get(pack, cues::IMPACT_BODY, false),
            crunch: t.get(pack, cues::IMPACT_CRUNCH, true),
        }
    }
}

/// Per-entity state carried between frames.
#[derive(Debug, Clone, Default)]
struct VehicleRuntime {
    car_id: String,
    /// Gear seen last frame (`None` until the first frame), for the shift clunk.
    last_gear: Option<i8>,
    engine: EngineAudioState,
    tires: TireAudioState,
    siren: SirenState,
    broken_glass: BTreeSet<BodyRegion>,
    /// Worst `RegionDamaged` severity among body panels (drives the rattle loop).
    panel_damage: f32,
    /// Engine layers kept by the LOD last frame (layer indices), for its hysteresis.
    engine_lod: Vec<usize>,
    /// Worst `RegionDamaged` severity per region (panel ring / buzz, horn health).
    region_damage: BTreeMap<BodyRegion, f32>,
    /// Deformation reservoir behind the crunch loop.
    crunch: CrunchState,
    /// Blow-offs fired so far (the physical engine voice fires one when it increments).
    blowoffs: u32,
}

/// How engines are voiced.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Default,
    serde::Serialize,
    serde::Deserialize,
)]
pub enum EngineModelMode {
    /// Recorded / procedural layers crossfaded by rpm (every car). The default, and the
    /// fallback when the backend has no physical engine synthesizer.
    #[default]
    Layers,
    /// The focus car and the nearest `engine.physical_max_others` cars within
    /// `engine.physical_radius_m` play the physically modelled engine (`engine/physical`, one
    /// voice per car); every other car keeps the layers.
    Physical,
}

/// Testbed / gameplay override of a vehicle's siren.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SirenOverride {
    /// Force the siren on even without `VehicleInput::siren`.
    pub on: bool,
    /// Mode to use instead of the car's default.
    pub mode: SirenMode,
}

/// Debug counters for the HUD.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize)]
pub struct DirectorStats {
    /// Playing looped voices.
    pub looped: usize,
    /// Sounding one-shots.
    pub one_shots: usize,
    /// Voices per bus.
    pub per_bus: BTreeMap<Bus, usize>,
    /// Desires culled last frame.
    pub culled: usize,
    /// Vehicles with audio state.
    pub vehicles: usize,
    /// Tracked contact pairs.
    pub contact_pairs: usize,
    /// Queued scanner lines.
    pub radio_queued: usize,
    /// Scanner lines dropped so far.
    pub radio_dropped: u32,
    /// Vehicles skipped because their car was never registered.
    pub unregistered: usize,
}

/// Turns simulation state and events into audio commands. See the crate docs.
pub struct AudioDirector {
    lib: AudioLibrary,
    cues: CueTable,
    cars: BTreeMap<String, CarAudio>,
    default_world: WorldCues,
    vehicles: BTreeMap<EntityId, VehicleRuntime>,
    pairs: BTreeMap<(u32, u32), PairState>,
    siren_overrides: BTreeMap<EntityId, SirenOverride>,
    mixer: VoiceMixer,
    scanner: Scanner,
    rng: Rng,
    seed: u64,
    last_tick: Option<u64>,
    /// Once `SimEvent::GlassShattered` has been seen, glass is no longer inferred from contacts.
    glass_events_seen: bool,
    /// Racers currently in a police pursuit (radio priority heuristic).
    pursuits: BTreeSet<EntityId>,
    one_shot_seq: u32,
    stats: DirectorStats,
    engine_model: EngineModelMode,
    /// Cars voiced by the physical engine model last frame (hysteresis).
    physical_set: BTreeSet<EntityId>,
    /// Installed sound-relevant mods per entity (exhaust, intake, kits).
    tunes: BTreeMap<EntityId, EngineTune>,
    /// Physical engine synth params per (car, tune) other than stock.
    tuned_specs: BTreeMap<(String, EngineTune), BTreeMap<String, f32>>,
    physical_cue: CueRef,
    /// Total plastic energy per vehicle given by the caller this frame
    /// ([`AudioDirector::set_plastic_energy`]), J.
    plastic_in: BTreeMap<EntityId, f32>,
    /// Vehicle whose cabin the listener is in this frame (interior camera).
    interior: Option<EntityId>,
}

/// Pack holding the physically modelled engine cue.
pub const PHYSICAL_ENGINE_PACK: &str = "engine_physical";

const DEFAULT_WORLD_PACK: &str = "world_proc";
const DEFAULT_SIREN_PACK: &str = "siren_proc";

impl AudioDirector {
    /// New director over `lib` with a seed for the (purely cosmetic) pitch variation.
    pub fn new(lib: AudioLibrary, seed: u64) -> Self {
        let mut cues = CueTable::from_library(&lib);
        let default_world = WorldCues::new(&mut cues, DEFAULT_WORLD_PACK);
        let physical_cue = cues.get(PHYSICAL_ENGINE_PACK, cues::ENGINE_PHYSICAL, true);
        Self {
            lib,
            cues,
            cars: BTreeMap::new(),
            default_world,
            vehicles: BTreeMap::new(),
            pairs: BTreeMap::new(),
            siren_overrides: BTreeMap::new(),
            mixer: VoiceMixer::default(),
            scanner: Scanner::default(),
            rng: Rng::new(seed),
            seed,
            last_tick: None,
            glass_events_seen: false,
            pursuits: BTreeSet::new(),
            one_shot_seq: 0,
            stats: DirectorStats::default(),
            engine_model: EngineModelMode::Layers,
            physical_set: BTreeSet::new(),
            tunes: BTreeMap::new(),
            tuned_specs: BTreeMap::new(),
            physical_cue,
            plastic_in: BTreeMap::new(),
            interior: None,
        }
    }

    /// Choose how engines are voiced (the backend switches to
    /// [`EngineModelMode::Physical`] once its engine synthesizer is running). Voices move
    /// over on the next frame.
    pub fn set_engine_model(&mut self, mode: EngineModelMode) {
        self.engine_model = mode;
    }

    /// Current engine voicing.
    pub fn engine_model(&self) -> EngineModelMode {
        self.engine_model
    }

    /// Installed sound-relevant mods of `entity` (louder / rawer exhaust, intake, kits).
    /// Stock when never set.
    pub fn set_engine_tune(&mut self, entity: EntityId, tune: EngineTune) {
        if tune.is_stock() {
            self.tunes.remove(&entity);
        } else {
            self.tunes.insert(entity, tune);
        }
    }

    /// The physical engine synth params a car is voiced with (`tune` applied), if registered.
    pub fn physical_engine_params(
        &mut self,
        car_id: &str,
        tune: EngineTune,
    ) -> Option<BTreeMap<String, f32>> {
        let car = self.cars.get(car_id)?;
        if tune.is_stock() {
            return Some(car.physical_stock.clone());
        }
        let key = (car_id.to_string(), tune);
        if let Some(p) = self.tuned_specs.get(&key) {
            return Some(p.clone());
        }
        let p = physical_spec(car_id, car, &tune).to_synth_params();
        self.tuned_specs.insert(key, p.clone());
        Some(p)
    }

    /// The library in use.
    pub fn library(&self) -> &AudioLibrary {
        &self.lib
    }

    /// Whether a car id is registered.
    pub fn has_car(&self, car_id: &str) -> bool {
        self.cars.contains_key(car_id)
    }

    /// Register a car (idempotent): resolves its engine layers, cues and lattice materials.
    pub fn register_car(&mut self, car: &LoadedCar) {
        if self.cars.contains_key(&car.def.id) {
            return;
        }
        let def = self.lib.car_or_fallback(&car.def);
        let e = &car.def.engine;
        let limits = EngineLimits {
            idle_rpm: e.idle_rpm,
            redline_rpm: e.redline_rpm,
            rev_limit_rpm: e.rev_limit_rpm,
        };
        let layers = match self.lib.packs.get(&def.engine_pack) {
            Some(p) => EngineLayers::resolve(p, e.idle_rpm, e.redline_rpm),
            None => EngineLayers { layers: Vec::new() },
        };
        let t = &mut self.cues;
        let ep = def.engine_pack.as_str();
        let layer_cues = layers
            .layers
            .iter()
            .map(|l| t.get(ep, &l.cue, true))
            .collect();
        let siren_pack = def.siren.as_ref().map(|s| s.pack.clone());
        let car_audio = CarAudio {
            limits,
            layer_cues,
            limiter: t.get(ep, cues::LIMITER, true),
            turbo: t.get(ep, cues::TURBO_WHISTLE, true),
            blowoff: t.get(ep, cues::TURBO_BLOWOFF, false),
            supercharger: t.get(ep, cues::SUPERCHARGER_WHINE, true),
            pop: t.get(ep, cues::EXHAUST_POP, false),
            horn: t.get(&def.world_pack, &def.horn_cue, true),
            horn_police: if def.horn_cue == cues::HORN_STANDARD {
                t.get(&def.world_pack, cues::HORN_POLICE, true)
            } else {
                t.get(&def.world_pack, &def.horn_cue, true)
            },
            siren: SirenPattern::ALL.map(|p| {
                t.get(
                    siren_pack.as_deref().unwrap_or(DEFAULT_SIREN_PACK),
                    p.cue(),
                    true,
                )
            }),
            world: WorldCues::new(t, &def.world_pack),
            materials: MaterialMap::from_lattice(&car.lattice, &car.materials),
            static_load_n: car.def.mass_kg * GRAVITY_MPS2 * 0.25,
            mass_kg: car.def.mass_kg,
            cabin: cabin_box(&car.lattice),
            wheel_radius_m: car.def.wheels.each_ref().map(|w| w.radius_m),
            class: car.def.class,
            final_drive: car.def.drivetrain.final_drive,
            physical_stock: BTreeMap::new(),
            layers,
            def,
        };
        let mut car_audio = car_audio;
        car_audio.physical_stock =
            physical_spec(&car.def.id, &car_audio, &EngineTune::default()).to_synth_params();
        self.cars.insert(car.def.id.clone(), car_audio);
    }

    /// Override a vehicle's siren (`None` = back to its input and car default).
    pub fn set_siren_override(&mut self, entity: EntityId, ov: Option<SirenOverride>) {
        match ov {
            Some(o) => {
                self.siren_overrides.insert(entity, o);
            }
            None => {
                self.siren_overrides.remove(&entity);
            }
        }
    }

    /// Scanner tier of the listener.
    pub fn set_scanner_tier(&mut self, tier: ScannerTier) {
        self.scanner.set_tier(tier);
    }

    /// Queue a police radio line (until `PoliceEvent::Radio` exists in the contract).
    pub fn push_radio_line(&mut self, line: RadioLine) {
        self.scanner.push(&self.lib.config.scanner, line);
    }

    /// Total plastic (deformation) energy of a vehicle's lattice, J, for the next
    /// [`update`](Self::update) (`wp_damage::DamageModel::total_plastic_energy_j`). The crunch
    /// loop follows its growth; without it the director estimates the plastic work of each hit
    /// from its collision energy.
    pub fn set_plastic_energy(&mut self, entity: EntityId, total_plastic_j: f32) {
        if total_plastic_j.is_finite() {
            self.plastic_in.insert(entity, total_plastic_j);
        }
    }

    /// Vehicle whose cabin the listener was in last frame (interior camera), if any.
    pub fn interior(&self) -> Option<EntityId> {
        self.interior
    }

    /// Debug counters from the last frame.
    pub fn stats(&self) -> &DirectorStats {
        &self.stats
    }

    /// Forget every transient state (session restart / replay seek) and stop all voices.
    pub fn reset(&mut self, out: &mut Vec<AudioCommand>) {
        self.mixer.stop_all(self.lib.config.stop_fade_s, out);
        self.scanner.clear(out);
        self.vehicles.clear();
        self.pairs.clear();
        self.pursuits.clear();
        self.last_tick = None;
        self.one_shot_seq = 0;
        self.rng = Rng::new(self.seed);
    }

    fn next_one_shot(&mut self, entity: u32) -> VoiceId {
        self.one_shot_seq = self.one_shot_seq.wrapping_add(1);
        VoiceId::new(entity, slot::one_shot(self.one_shot_seq))
    }

    /// Produce this frame's commands.
    pub fn update(&mut self, frame: &AudioFrame) -> Vec<AudioCommand> {
        let mut out = Vec::new();
        if self.last_tick.is_some_and(|t| frame.tick < t) {
            // Time went backwards: new session or replay seek.
            self.reset(&mut out);
        }
        let dt = match self.last_tick {
            Some(t) => ((frame.tick - t) as f32 * PHYSICS_DT).min(0.25),
            None => 0.0,
        };
        self.last_tick = Some(frame.tick);
        let tick = frame.tick;

        let mut vehicles: Vec<&VehicleAudioInput> = frame.vehicles.iter().collect();
        vehicles.sort_by_key(|v| v.entity);
        let by_id: BTreeMap<EntityId, &VehicleAudioInput> =
            vehicles.iter().map(|v| (v.entity, *v)).collect();

        // Drop state of vehicles that left (their voices stop in the mixer diff).
        self.vehicles.retain(|id, _| by_id.contains_key(id));
        self.interior = frame.focus.and_then(|f| {
            let v = by_id.get(&f)?;
            let (lo, hi) = self.cars.get(v.car_id)?.cabin?;
            let p = world_to_local(v.state.pos, v.state.rot, frame.listener.pos);
            (0..3).all(|i| p[i] >= lo[i] && p[i] <= hi[i]).then_some(f)
        });
        let plastic_in = std::mem::take(&mut self.plastic_in);

        self.select_physical(&vehicles, frame.focus, frame.listener.pos);
        let mut desires: Vec<Desire> = Vec::new();
        let mut unregistered = 0;
        for v in &vehicles {
            if !self.cars.contains_key(v.car_id) {
                unregistered += 1;
                continue;
            }
            let physical = self.physical_set.contains(&v.entity);
            self.vehicle_desires(
                v,
                frame.focus == Some(v.entity),
                physical,
                frame.listener.pos,
                tick,
                dt,
                &mut desires,
            );
        }
        self.event_desires(frame.events, &by_id, tick, &mut desires);
        self.scrape_desires(&by_id, tick, &mut desires);
        // Crunch after the events (they add estimated plastic work for unmeasured cars).
        for v in &vehicles {
            if self.cars.contains_key(v.car_id) {
                self.crunch_desire(v, plastic_in.get(&v.entity).copied(), dt, &mut desires);
            }
        }
        if let Some(own) = self.interior {
            cabin_muffle(own, &mut desires);
        }

        let cfg = &self.lib.config;
        let ms: MixerStats =
            self.mixer
                .mix(cfg, &self.cues, &frame.listener, tick, desires, &mut out);
        self.scanner.update(&cfg.scanner, tick, &mut out);
        self.stats = DirectorStats {
            looped: ms.looped,
            one_shots: ms.one_shots,
            per_bus: ms.per_bus,
            culled: ms.culled,
            vehicles: self.vehicles.len(),
            contact_pairs: self.pairs.len(),
            radio_queued: self.scanner.queued(),
            radio_dropped: self.scanner.dropped,
            unregistered,
        };
        out
    }

    /// Pick the cars voiced by the physical engine model this frame: the focus car plus the
    /// nearest `physical_max_others` within `physical_radius_m` (a car already voiced ranks
    /// 20 % closer and stays out to 1.25 × the radius, so the set does not flicker).
    fn select_physical(
        &mut self,
        vehicles: &[&VehicleAudioInput],
        focus: Option<EntityId>,
        listener_pos: [f32; 3],
    ) {
        if self.engine_model != EngineModelMode::Physical {
            self.physical_set.clear();
            return;
        }
        let cfg = &self.lib.config.engine;
        let r = cfg.physical_radius_m;
        let mut ranked: Vec<(f32, EntityId)> = Vec::new();
        let mut set = BTreeSet::new();
        for v in vehicles {
            if !self.cars.contains_key(v.car_id) {
                continue;
            }
            if Some(v.entity) == focus {
                set.insert(v.entity);
                continue;
            }
            let d = len(sub(v.state.pos, listener_pos));
            let was = self.physical_set.contains(&v.entity);
            let (limit, rank) = if was { (r * 1.25, d * 0.8) } else { (r, d) };
            if d.is_finite() && d < limit {
                ranked.push((rank, v.entity));
            }
        }
        ranked.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        set.extend(ranked.iter().take(cfg.physical_max_others).map(|x| x.1));
        self.physical_set = set;
    }

    #[allow(clippy::too_many_arguments)]
    fn vehicle_desires(
        &mut self,
        v: &VehicleAudioInput,
        focus: bool,
        physical: bool,
        listener_pos: [f32; 3],
        tick: u64,
        dt: f32,
        out: &mut Vec<Desire>,
    ) {
        let tune = self.tunes.get(&v.entity).copied();
        let physical_params = if physical {
            self.physical_engine_params(v.car_id, tune.unwrap_or_default())
        } else {
            None
        };
        let cfg = &self.lib.config;
        let car = &self.cars[v.car_id];
        let rt = self.vehicles.entry(v.entity).or_default();
        if rt.car_id != v.car_id {
            *rt = VehicleRuntime {
                car_id: v.car_id.to_string(),
                ..Default::default()
            };
        }
        let s = v.state;
        let e = v.entity.0;
        let pos = s.pos;
        let vel = s.lin_vel;
        let up = |p: Priority| if focus { Priority::Critical.max(p) } else { p };

        // Engine (silent once the engine component has failed).
        let engine_alive = s
            .component_health
            .get(&Component::Engine)
            .is_none_or(|h| *h > 0.0);
        if engine_alive {
            let tuned_def;
            let engine_def = match tune {
                Some(t) => {
                    tuned_def = t.apply(&car.def.engine);
                    &tuned_def
                }
                None => &car.def.engine,
            };
            let eo = update_engine(
                &mut rt.engine,
                &cfg.engine,
                engine_def,
                car.limits,
                &car.layers,
                EngineInput {
                    rpm: s.engine_rpm,
                    pedal: v.throttle,
                    applied: v.throttle_applied.unwrap_or(v.throttle),
                },
                dt,
                &mut self.rng,
            );
            let prio = if focus {
                Priority::Critical
            } else if v.kind == DriverKind::Traffic {
                Priority::Low
            } else {
                Priority::Normal
            };
            let kind_gain = if v.kind == DriverKind::Traffic {
                cfg.engine.traffic_gain
            } else {
                1.0
            };
            if eo.blowoff.is_some() {
                rt.blowoffs = rt.blowoffs.wrapping_add(1);
            }
            let voiced_physically = physical_params.is_some();
            if let Some(params) = physical_params {
                // One voice: the synthesizer makes the firing pulses, exhaust, intake,
                // mechanical noise, limiter, turbo, blow-off and crackle itself.
                let mut d = Desire::new(
                    VoiceId::new(e, slot::ENGINE_PHYSICAL),
                    Bus::Engine,
                    self.physical_cue,
                    prio,
                    pos,
                    vel,
                );
                d.gain = cfg.engine.gain * engine_def.gain * kind_gain;
                d.freq_hz = engine_def.firing_hz(s.engine_rpm);
                d.load = eo.load;
                d.synth = Some(params);
                d.engine = Some(EngineFrame {
                    rpm: s.engine_rpm.max(0.0),
                    throttle: clamp01(v.throttle_applied.unwrap_or(v.throttle)),
                    pedal: clamp01(v.throttle),
                    gear: s.gear as f32,
                    speed_mps: s.speed_mps.abs(),
                    boost: rt.engine.boost,
                    health: s
                        .component_health
                        .get(&Component::Engine)
                        .copied()
                        .unwrap_or(1.0),
                    blowoffs: rt.blowoffs as f32,
                });
                out.push(d);
            }
            if !voiced_physically {
                // Engine LOD (playtest #47): the focus car keeps every layer; other cars keep the
                // crossfade pair near the listener and one layer farther out, and their second
                // layer yields to every other car's main layer when voices run out.
                let layers = if focus {
                    eo.layers.clone()
                } else {
                    let d = [
                        pos[0] - listener_pos[0],
                        pos[1] - listener_pos[1],
                        pos[2] - listener_pos[2],
                    ];
                    let near = d[0] * d[0] + d[1] * d[1] + d[2] * d[2]
                        < cfg.engine.lod_two_layer_m * cfg.engine.lod_two_layer_m;
                    let kept = engine_lod(&eo.layers, if near { 2 } else { 1 }, &rt.engine_lod);
                    rt.engine_lod = kept.iter().map(|l| l.0).collect();
                    kept
                };
                for (rank, (i, gain, pitch)) in layers.iter().enumerate() {
                    let l = &car.layers.layers[*i];
                    let mut d = Desire::new(
                        VoiceId::new(e, slot::ENGINE_LAYER + l.index as u32),
                        Bus::Engine,
                        car.layer_cues[*i],
                        prio,
                        pos,
                        vel,
                    );
                    d.gain = *gain * kind_gain;
                    d.pitch = *pitch;
                    d.freq_hz = eo.firing_hz;
                    d.load = eo.load;
                    d.synth = Some(engine_synth(&car.def.engine, l.root_rpm));
                    if !focus && rank > 0 {
                        d.cull_scale = cfg.engine.second_layer_cull_scale;
                    }
                    out.push(d);
                }
                if eo.limiter > 1e-3 {
                    let mut d = Desire::new(
                        VoiceId::new(e, slot::ENGINE_LIMITER),
                        Bus::Engine,
                        car.limiter,
                        prio,
                        pos,
                        vel,
                    );
                    d.gain = eo.limiter * kind_gain;
                    d.freq_hz = eo.firing_hz;
                    d.load = 1.0;
                    d.synth = Some(engine_synth(&car.def.engine, car.limits.rev_limit_rpm));
                    out.push(d);
                }
                for (slot_code, cue, v_) in [
                    (slot::TURBO, car.turbo, eo.turbo),
                    (slot::SUPERCHARGER, car.supercharger, eo.supercharger),
                ] {
                    if let Some((gain, pitch)) = v_ {
                        let mut d = Desire::new(
                            VoiceId::new(e, slot_code),
                            Bus::Engine,
                            cue,
                            up(Priority::Low),
                            pos,
                            vel,
                        );
                        d.gain = gain;
                        d.pitch = pitch;
                        d.load = pitch;
                        out.push(d);
                    }
                }
                for (cue, g) in [(car.blowoff, eo.blowoff), (car.pop, eo.pop)] {
                    if let Some(g) = g {
                        self.one_shot_seq = self.one_shot_seq.wrapping_add(1);
                        let mut d = Desire::new(
                            VoiceId::new(e, slot::one_shot(self.one_shot_seq)),
                            Bus::Engine,
                            cue,
                            up(Priority::Normal),
                            pos,
                            vel,
                        );
                        d.gain = g;
                        d.pitch = self.rng.range_f32(0.92, 1.08);
                        out.push(d);
                    }
                }
            }
        }
        // Cooling damage hiss.
        if let Some(h) = s.component_health.get(&Component::Radiator) {
            let g = clamp01((0.5 - h) * 2.0);
            if g > 0.0 {
                let mut d = Desire::new(
                    VoiceId::new(e, slot::STEAM),
                    Bus::World,
                    car.world.steam,
                    up(Priority::Low),
                    pos,
                    vel,
                );
                d.gain = 0.4 * g;
                d.load = g;
                out.push(d);
            }
        }

        // Gear-shift clunk, only when the world pack defines `drivetrain/shift`.
        let shifted = rt.last_gear.is_some_and(|g| g != s.gear);
        rt.last_gear = Some(s.gear);
        if shifted && self.cues.info(car.world.shift).defined {
            self.one_shot_seq = self.one_shot_seq.wrapping_add(1);
            let d = Desire::new(
                VoiceId::new(e, slot::one_shot(self.one_shot_seq)),
                Bus::World,
                car.world.shift,
                up(Priority::Low),
                pos,
                vel,
            );
            out.push(d);
        }

        // Tires, surfaces, suspension.
        let to = update_tires(
            &mut rt.tires,
            &cfg.tires,
            s,
            car.static_load_n,
            car.wheel_radius_m,
            v.road_rumble,
            tick,
            dt,
        );
        if let Some((gain, pitch, k)) = to.squeal {
            let mut d = Desire::new(
                VoiceId::new(e, slot::SQUEAL),
                Bus::World,
                car.world.squeal,
                up(Priority::Normal),
                pos,
                vel,
            );
            d.gain = gain;
            d.pitch = pitch;
            d.load = k;
            out.push(d);
        }
        for (slot_code, cue, spin) in [
            (slot::SPIN, car.world.spin, to.spin),
            (slot::SPIN_LOOSE, car.world.spin_loose, to.spin_loose),
        ] {
            if let Some((gain, pitch, k)) = spin {
                let mut d = Desire::new(
                    VoiceId::new(e, slot_code),
                    Bus::World,
                    cue,
                    up(Priority::Normal),
                    pos,
                    vel,
                );
                d.gain = gain;
                d.pitch = pitch;
                d.load = k;
                out.push(d);
            }
        }
        if let Some((gain, k)) = to.scrub {
            let mut d = Desire::new(
                VoiceId::new(e, slot::SCRUB),
                Bus::World,
                car.world.scrub,
                up(Priority::Low),
                pos,
                vel,
            );
            d.gain = gain;
            d.load = k;
            out.push(d);
        }
        if let Some((idx, gain, frac)) = to.surface {
            let cue = car.world.surfaces[idx];
            let speed = len(vel);
            let pitch = match self.cues.info(cue).root_speed_mps {
                Some(root) if root > 0.0 => (speed / root).clamp(0.3, 3.0),
                _ => 0.6 + 0.8 * frac,
            };
            let mut d = Desire::new(
                VoiceId::new(e, slot::SURFACE + idx as u32),
                Bus::World,
                cue,
                up(Priority::Low),
                pos,
                vel,
            );
            d.gain = gain;
            d.pitch = pitch;
            d.load = frac;
            out.push(d);
        }
        let thumps = to.thumps;
        // Damaged panels rattle with speed and road texture.
        if rt.panel_damage > 0.1 {
            let g =
                0.35 * rt.panel_damage * clamp01(len(vel) / 20.0) * (0.5 + clamp01(v.road_rumble));
            let mut d = Desire::new(
                VoiceId::new(e, slot::BODY_RATTLE),
                Bus::World,
                car.world.rattle,
                up(Priority::Low),
                pos,
                vel,
            );
            d.gain = g;
            d.load = rt.panel_damage;
            d.pitch = 0.8 + 0.4 * clamp01(len(vel) / 30.0);
            out.push(d);
        }
        // Horn. Police units get the air horn when their car has a standard horn; a smashed
        // front end (the horn sits behind the grille) or a dying battery makes it weak and
        // rattly: `load` = horn damage 0..1.
        if v.horn {
            let cue = if v.kind == DriverKind::Police && self.cues.info(car.horn_police).defined {
                car.horn_police
            } else {
                car.horn
            };
            let mut d = Desire::new(
                VoiceId::new(e, slot::HORN),
                Bus::World,
                cue,
                up(Priority::High),
                pos,
                vel,
            );
            d.gain = cfg.sirens.horn_gain;
            d.load = horn_damage(&rt.region_damage, s);
            out.push(d);
        }
        // Siren.
        let ov = self.siren_overrides.get(&v.entity).copied();
        let on = v.siren || ov.is_some_and(|o| o.on);
        let police = v.kind == DriverKind::Police;
        let fitted = car.def.siren.is_some() || ov.is_some() || police;
        let mode = match (ov, &car.def.siren) {
            (Some(o), _) => o.mode,
            (None, Some(sd)) => sd.default_mode,
            (None, None) if police => SirenMode::Auto,
            (None, None) => SirenMode::Off,
        };
        if fitted {
            if let Some(so) = update_siren(&mut rt.siren, &cfg.sirens, mode, on, s.speed_mps, tick)
            {
                {
                    let sc = car.siren;
                    let mut d = Desire::new(
                        VoiceId::new(e, slot::SIREN + so.pattern.index()),
                        Bus::Police,
                        sc[so.pattern.index() as usize],
                        Priority::High,
                        pos,
                        vel,
                    );
                    d.gain = cfg.sirens.gain;
                    d.freq_hz = so.freq_hz;
                    d.phase = so.phase;
                    d.synth = Some(so.cadence.synth_params());
                    out.push(d);
                }
            }
        }
        let world_thump = car.world.thump;
        let world_land = car.world.land;
        for (wheel, gain, landing) in thumps {
            let id = self.next_one_shot(e);
            let cue = if landing { world_land } else { world_thump };
            let mut d = Desire::new(id, Bus::World, cue, up(Priority::Low), pos, vel);
            d.gain = gain;
            d.pitch = self.rng.range_f32(0.9, 1.1) * if wheel < 2 { 1.0 } else { 0.95 };
            out.push(d);
        }
    }

    fn event_desires(
        &mut self,
        events: &[SimEvent],
        by_id: &BTreeMap<EntityId, &VehicleAudioInput>,
        tick: u64,
        out: &mut Vec<Desire>,
    ) {
        let mix = self.lib.config.impacts.clone();
        let debounce = (mix.debounce_s / PHYSICS_DT) as u64;
        for ev in events {
            match ev {
                SimEvent::Contact {
                    a,
                    b,
                    point,
                    normal,
                    impulse_ns,
                    rel_speed,
                } => {
                    let va = by_id
                        .get(a)
                        .filter(|v| self.cars.contains_key(v.car_id))
                        .copied();
                    let vb = by_id
                        .get(b)
                        .filter(|v| self.cars.contains_key(v.car_id))
                        .copied();
                    // Material per vehicle side.
                    // `out` = the contact normal pointing out of that vehicle (the event's
                    // normal points from `a` toward `b`).
                    let side = |v: Option<&VehicleAudioInput>, out: [f32; 3]| {
                        v.and_then(|v| {
                            let car = &self.cars[v.car_id];
                            let local = world_to_local(v.state.pos, v.state.rot, *point);
                            let n_local = world_to_local([0.0; 3], v.state.rot, out);
                            car.materials
                                .at(local)
                                .map(|(k, r)| struck_surface(&car.materials, local, n_local, k, r))
                                .map(|(k, r)| (v.entity, k, r))
                        })
                    };
                    let n = *normal;
                    let sides: Vec<_> = [side(va, n), side(vb, [-n[0], -n[1], -n[2]])]
                        .into_iter()
                        .flatten()
                        .collect();
                    let owner = va.or(vb);
                    let owner_id = owner.map(|v| v.entity.0).unwrap_or(u32::MAX);
                    let world = match owner {
                        Some(v) => self.cars[v.car_id].world.clone(),
                        None => self.default_world.clone(),
                    };

                    let key = pair_key(a.0, b.0);
                    let pst = self.pairs.entry(key).or_default();
                    pst.last_contact_tick = tick;
                    pst.point = *point;
                    // Scrape from tangential relative velocity.
                    let vel_of = |v: Option<&VehicleAudioInput>| {
                        v.map(|v| v.state.lin_vel).unwrap_or([0.0; 3])
                    };
                    let vr = sub(vel_of(va), vel_of(vb));
                    let vn = dot(vr, *normal);
                    let vt = len(sub(vr, [normal[0] * vn, normal[1] * vn, normal[2] * vn]));
                    let target = clamp01(
                        (vt - mix.scrape_min_speed_mps)
                            / (mix.scrape_speed_ref_mps - mix.scrape_min_speed_mps).max(0.1),
                    );
                    pst.frame_scrape = pst.frame_scrape.max(target);
                    pst.slide_mps = pst.slide_mps.max(vt);
                    pst.force = pst
                        .force
                        .max(scrape_force(&self.lib.config.impact_synth, *impulse_ns));
                    pst.world =
                        *b == wp_contracts::WORLD_ENTITY || *a == wp_contracts::WORLD_ENTITY;
                    pst.metal = !sides
                        .iter()
                        .all(|s| s.1 == wp_contracts::MaterialKind::Plastic)
                        || sides.is_empty();

                    // Impact.
                    if *rel_speed >= mix.min_speed_mps
                        && pst.should_impact(&mix, *rel_speed, tick, debounce)
                    {
                        pst.last_impact_tick = Some(tick);
                        pst.last_impact_speed = *rel_speed;
                        let mut layers: BTreeSet<ImpactLayer> = BTreeSet::new();
                        if sides.is_empty() {
                            layers.extend(impact_layers(
                                &mix,
                                wp_contracts::MaterialKind::Panel,
                                *rel_speed,
                                false,
                            ));
                        }
                        let mut shattered = Vec::new();
                        for (ent, kind, region) in &sides {
                            let intact = !self.glass_events_seen
                                && !self
                                    .vehicles
                                    .get(ent)
                                    .is_some_and(|r| r.broken_glass.contains(region));
                            let ls = impact_layers(&mix, *kind, *rel_speed, intact);
                            if ls.contains(&ImpactLayer::GlassShatter) {
                                shattered.push((*ent, *region));
                            }
                            layers.extend(ls);
                        }
                        for (ent, region) in shattered {
                            if let Some(r) = self.vehicles.get_mut(&ent) {
                                r.broken_glass.insert(region);
                            }
                            let strength = clamp01(*rel_speed / 15.0);
                            self.glass_body(&world, ent, region, *point, [0.0; 3], strength, out);
                        }
                        let g = impact_gain(&mix, *impulse_ns, *rel_speed);
                        self.body_hits(
                            &sides,
                            by_id,
                            *point,
                            *impulse_ns,
                            *rel_speed,
                            [va.map(|v| v.entity), vb.map(|v| v.entity)],
                            g,
                            &world,
                            out,
                        );
                        for l in layers {
                            let id = self.next_one_shot(owner_id);
                            let prio = if l == ImpactLayer::GlassShatter {
                                Priority::Critical
                            } else {
                                Priority::High
                            };
                            let mut d = Desire::new(
                                id,
                                Bus::World,
                                world.impacts[&l],
                                prio,
                                *point,
                                [0.0; 3],
                            );
                            d.gain = g;
                            d.pitch = self.rng.range_f32(0.9, 1.1);
                            d.load = clamp01(*rel_speed / 20.0);
                            out.push(d);
                        }
                    }
                }
                SimEvent::PartDetached { vehicle, .. } => {
                    self.vehicle_one_shot(by_id, *vehicle, |w| w.part_tear, 0.9, out)
                }
                SimEvent::WheelDetached { vehicle, .. } => {
                    self.vehicle_one_shot(by_id, *vehicle, |w| w.wheel_off, 1.0, out)
                }
                SimEvent::ComponentFailed {
                    vehicle,
                    component: Component::Engine,
                } => self.vehicle_one_shot(by_id, *vehicle, |w| w.engine_fail, 1.0, out),
                SimEvent::TotalLoss { vehicle } => self.vehicle_one_shot(
                    by_id,
                    *vehicle,
                    |w| w.impacts[&ImpactLayer::StructuralCrunch],
                    1.2,
                    out,
                ),
                SimEvent::GlassShattered { vehicle, region } => {
                    self.glass_events_seen = true;
                    self.glass_shattered(by_id, *vehicle, *region, out);
                }
                SimEvent::RegionDamaged {
                    vehicle,
                    region,
                    severity,
                } => {
                    if let Some(r) = self.vehicles.get_mut(vehicle) {
                        let sev = clamp01(*severity);
                        let e = r.region_damage.entry(*region).or_insert(0.0);
                        *e = e.max(sev);
                        if is_panel(*region) {
                            r.panel_damage = r.panel_damage.max(sev);
                        }
                    }
                }
                SimEvent::Police(_) => self.police_event(ev, by_id, tick, out),
                _ => {}
            }
        }
    }

    /// Glass of `region` broke (damage event): shatter at the region, once.
    fn glass_shattered(
        &mut self,
        by_id: &BTreeMap<EntityId, &VehicleAudioInput>,
        vehicle: EntityId,
        region: BodyRegion,
        out: &mut Vec<Desire>,
    ) {
        let Some(v) = by_id.get(&vehicle) else {
            return;
        };
        let Some(car) = self.cars.get(v.car_id) else {
            return;
        };
        if let Some(r) = self.vehicles.get_mut(&vehicle) {
            if !r.broken_glass.insert(region) {
                return;
            }
        }
        let local = car.materials.region_centroid(region).unwrap_or([0.0; 3]);
        let o = rotate(v.state.rot, local);
        let p = v.state.pos;
        let point = [p[0] + o[0], p[1] + o[1], p[2] + o[2]];
        let cue = car.world.impacts[&ImpactLayer::GlassShatter];
        let world = car.world.clone();
        let vel = v.state.lin_vel;
        let id = self.next_one_shot(vehicle.0);
        let mut d = Desire::new(id, Bus::World, cue, Priority::Critical, point, vel);
        d.gain = self.lib.config.impacts.impact_gain;
        d.pitch = self.rng.range_f32(0.92, 1.08);
        out.push(d);
        self.glass_body(&world, vehicle, region, point, vel, 1.0, out);
    }

    /// Synthesised shatter onset + debris tail for broken glass (when the pack has `impact/body`).
    #[allow(clippy::too_many_arguments)]
    fn glass_body(
        &mut self,
        world: &WorldCues,
        vehicle: EntityId,
        region: BodyRegion,
        point: [f32; 3],
        vel: [f32; 3],
        strength: f32,
        out: &mut Vec<Desire>,
    ) {
        if !self.cues.info(world.body).defined {
            return;
        }
        let id = self.next_one_shot(vehicle.0);
        let seed = cosmetic_seed(self.one_shot_seq);
        let mut d = Desire::new(id, Bus::World, world.body, Priority::Critical, point, vel);
        d.gain = self.lib.config.impacts.impact_gain * (0.6 + 0.4 * clamp01(strength));
        d.load = clamp01(strength);
        d.synth = Some(glass_params(
            region,
            strength,
            self.interior == Some(vehicle),
            seed,
        ));
        out.push(d);
    }

    /// One synthesised `impact/body` voice per struck vehicle of a hit (both cars ring in a
    /// car-to-car crash); world geometry alone gets one generic voice. Also books the hit's
    /// estimated plastic work for cars whose deformation energy the caller does not measure.
    #[allow(clippy::too_many_arguments)]
    fn body_hits(
        &mut self,
        sides: &[(EntityId, wp_contracts::MaterialKind, BodyRegion)],
        by_id: &BTreeMap<EntityId, &VehicleAudioInput>,
        point: [f32; 3],
        impulse_ns: f32,
        speed: f32,
        ents: [Option<EntityId>; 2],
        gain: f32,
        world: &WorldCues,
        out: &mut Vec<Desire>,
    ) {
        let syn = self.lib.config.impact_synth.clone();
        let cars = &self.cars;
        let mass_of = |e: Option<EntityId>| {
            e.and_then(|e| by_id.get(&e))
                .and_then(|v| cars.get(v.car_id))
                .map(|c| c.mass_kg)
        };
        let side_mass: Vec<Option<f32>> = sides.iter().map(|s| mass_of(Some(s.0))).collect();
        let m_eff = reduced_mass(mass_of(ents[0]), mass_of(ents[1]));
        let collision_j = 0.5 * m_eff * speed * speed;
        let n_sides = sides.len().max(1) as f32;
        for e in ents.into_iter().flatten() {
            if !self.plastic_in.contains_key(&e) {
                if let Some(r) = self.vehicles.get_mut(&e) {
                    r.crunch
                        .add_estimate(collision_j * syn.plastic_share_estimate / n_sides);
                }
            }
        }
        if !self.cues.info(world.body).defined {
            return;
        }
        let mut hits: Vec<(u32, Hit)> = Vec::new();
        for ((ent, kind, region), mass) in sides.iter().zip(side_mass) {
            let rt = self.vehicles.get(ent);
            let health = by_id.get(ent).map(|v| v.state.health).unwrap_or(1.0);
            let dmg = rt
                .and_then(|r| r.region_damage.get(region).copied())
                .unwrap_or(0.0)
                .max(0.5 * (1.0 - clamp01(health)));
            hits.push((
                ent.0,
                Hit {
                    region: Some(*region),
                    material: *kind,
                    speed_mps: speed,
                    impulse_ns,
                    eff_mass_kg: m_eff,
                    mass_kg: mass.unwrap_or(syn.ref_mass_kg),
                    panel_damage: dmg,
                    interior: self.interior == Some(*ent),
                    seed: 0,
                },
            ));
        }
        if hits.is_empty() {
            hits.push((
                ents.into_iter()
                    .flatten()
                    .next()
                    .map(|e| e.0)
                    .unwrap_or(u32::MAX),
                Hit {
                    region: None,
                    material: wp_contracts::MaterialKind::Panel,
                    speed_mps: speed,
                    impulse_ns,
                    eff_mass_kg: m_eff,
                    mass_kg: syn.ref_mass_kg,
                    panel_damage: 0.0,
                    interior: false,
                    seed: 0,
                },
            ));
        }
        for (owner, mut h) in hits {
            let id = self.next_one_shot(owner);
            h.seed = cosmetic_seed(self.one_shot_seq);
            let params = body_params(&syn, &h);
            let mut d = Desire::new(id, Bus::World, world.body, Priority::High, point, [0.0; 3]);
            d.gain = gain;
            d.load = params.get("strength").copied().unwrap_or(0.0);
            d.synth = Some(params);
            out.push(d);
        }
    }

    /// The granular crunch loop of one vehicle: on while its lattice is being crushed.
    fn crunch_desire(
        &mut self,
        v: &VehicleAudioInput,
        measured_j: Option<f32>,
        dt: f32,
        out: &mut Vec<Desire>,
    ) {
        let cfg = &self.lib.config.impact_synth;
        let Some(car) = self.cars.get(v.car_id) else {
            return;
        };
        let Some(rt) = self.vehicles.get_mut(&v.entity) else {
            return;
        };
        if let Some(j) = measured_j {
            rt.crunch.measure(j);
        }
        let level = rt.crunch.step(cfg, dt);
        if level <= 0.0 || !self.cues.info(car.world.crunch).defined {
            return;
        }
        let mut d = Desire::new(
            VoiceId::new(v.entity.0, slot::CRUNCH),
            Bus::World,
            car.world.crunch,
            Priority::High,
            v.state.pos,
            v.state.lin_vel,
        );
        d.gain = cfg.crunch_gain * (0.35 + 0.65 * level);
        d.load = level;
        // Heavier cars: bigger, lower-pitched crumpling.
        d.pitch = crate::impact::det_pow(
            self.lib.config.impact_synth.ref_mass_kg / car.mass_kg.max(100.0),
            0.25,
        )
        .clamp(0.75, 1.2);
        d.synth = Some(BTreeMap::from([(
            "interior".to_string(),
            if self.interior == Some(v.entity) {
                1.0
            } else {
                0.0
            },
        )]));
        out.push(d);
    }

    /// `wp_police` events: radio lines into the scanner, pursuit tracking, spike blow-outs.
    fn police_event(
        &mut self,
        ev: &SimEvent,
        by_id: &BTreeMap<EntityId, &VehicleAudioInput>,
        tick: u64,
        out: &mut Vec<Desire>,
    ) {
        let SimEvent::Police(pe) = ev else {
            return;
        };
        let pe = pe.clone();
        match pe {
            PoliceEvent::Radio { unit, text } => {
                let priority = match unit {
                    None => RadioPriority::Dispatch,
                    Some(_) if !self.pursuits.is_empty() => RadioPriority::Pursuit,
                    Some(_) => RadioPriority::Routine,
                };
                let line = RadioLine {
                    unit: unit.map(|u| u.0).unwrap_or(0),
                    text,
                    priority,
                    emitted_tick: tick,
                };
                self.scanner.push(&self.lib.config.scanner, line);
            }
            PoliceEvent::PursuitStarted { racer, .. } => {
                self.pursuits.insert(racer);
            }
            PoliceEvent::PursuitEnded { racer, .. } => {
                self.pursuits.remove(&racer);
            }
            PoliceEvent::SpikeHit { vehicle, wheels } => {
                for _ in wheels.iter().filter(|w| **w) {
                    self.vehicle_one_shot(by_id, vehicle, |w| w.tire_burst, 1.0, out);
                }
            }
            _ => {}
        }
    }

    fn vehicle_one_shot(
        &mut self,
        by_id: &BTreeMap<EntityId, &VehicleAudioInput>,
        vehicle: EntityId,
        cue: impl Fn(&WorldCues) -> CueRef,
        gain: f32,
        out: &mut Vec<Desire>,
    ) {
        let Some(v) = by_id.get(&vehicle) else {
            return;
        };
        let world = match self.cars.get(v.car_id) {
            Some(c) => &c.world,
            None => &self.default_world,
        };
        let cue = cue(world);
        let id = self.next_one_shot(vehicle.0);
        let mut d = Desire::new(
            id,
            Bus::World,
            cue,
            Priority::High,
            v.state.pos,
            v.state.lin_vel,
        );
        d.gain = gain;
        d.pitch = self.rng.range_f32(0.92, 1.08);
        out.push(d);
    }

    fn scrape_desires(
        &mut self,
        by_id: &BTreeMap<EntityId, &VehicleAudioInput>,
        tick: u64,
        out: &mut Vec<Desire>,
    ) {
        let mix = &self.lib.config.impacts;
        let release = (mix.scrape_release_s / PHYSICS_DT).max(1.0);
        let keep = (2.0 / PHYSICS_DT) as u64;
        self.pairs.retain(|_, p| tick <= p.last_contact_tick + keep);
        for (&(lo, hi), p) in self.pairs.iter_mut() {
            let age = tick.saturating_sub(p.last_contact_tick) as f32;
            if age == 0.0 {
                p.scrape = p.frame_scrape;
                p.frame_force = p.force;
            }
            p.frame_scrape = 0.0;
            p.force = 0.0;
            if age > 0.0 {
                p.slide_mps *= 0.9;
            }
            let level = p.scrape * clamp01(1.0 - age / release);
            if level < 0.02 {
                continue;
            }
            // The voice belongs to the lower vehicle id that is a vehicle (else the world).
            let (owner, other) = if by_id.contains_key(&EntityId(lo)) {
                (lo, hi)
            } else {
                (hi, lo)
            };
            let world = by_id
                .get(&EntityId(owner))
                .and_then(|v| self.cars.get(v.car_id))
                .map(|c| &c.world)
                .unwrap_or(&self.default_world);
            let cue = if p.metal {
                world.scrape_metal
            } else {
                world.scrape_plastic
            };
            let mut d = Desire::new(
                VoiceId::new(owner, slot::scrape(other)),
                Bus::World,
                cue,
                Priority::High,
                p.point,
                [0.0; 3],
            );
            d.gain = mix.scrape_gain * level;
            d.pitch = 0.8 + 0.4 * level;
            d.load = level;
            // Friction synthesis: stick-slip rate from the sliding speed, pressure from the
            // contact impulse; metal on the road / walls throws sparks.
            let syn = &self.lib.config.impact_synth;
            d.freq_hz = scrape_rate_hz(syn, p.slide_mps);
            d.phase = 0.0;
            d.synth = Some(BTreeMap::from([
                ("friction".to_string(), 1.0),
                ("metal".to_string(), if p.metal { 1.0 } else { 0.0 }),
                (
                    "sparks".to_string(),
                    if p.metal && p.world { 1.0 } else { 0.0 },
                ),
                ("force".to_string(), p.frame_force.max(0.15)),
            ]));
            out.push(d);
        }
    }
}

/// Outer body panels (they rattle when damaged; structure and glass do not).
fn is_panel(r: BodyRegion) -> bool {
    !matches!(
        r,
        BodyRegion::Windshield
            | BodyRegion::RearGlass
            | BodyRegion::FrameFront
            | BodyRegion::FrameRear
            | BodyRegion::FrameLeft
            | BodyRegion::FrameRight
            | BodyRegion::Cabin
    )
}

/// The physical engine description of a registered car with `tune` applied.
fn physical_spec(car_id: &str, car: &CarAudio, tune: &EngineTune) -> EngineVoiceSpec {
    // Driven wheels: the rear pair's radius (the differential the cabin hears most).
    let r = 0.5 * (car.wheel_radius_m[2] + car.wheel_radius_m[3]);
    EngineVoiceSpec::new(
        car_id,
        &car.def.engine,
        car.class,
        car.limits,
        car.final_drive,
        r,
        tune,
    )
}

/// Chassis-space box of the cabin from the lattice: the `Cabin` nodes, up to the roof, shrunk a
/// little so a camera on the hood or the bumper stays outside.
fn cabin_box(lattice: &wp_contracts::LatticeDef) -> Option<([f32; 3], [f32; 3])> {
    let mut lo = [f32::INFINITY; 3];
    let mut hi = [f32::NEG_INFINITY; 3];
    let mut any = false;
    for n in &lattice.nodes {
        if matches!(n.region, BodyRegion::Cabin | BodyRegion::Roof) {
            any = true;
            for i in 0..3 {
                lo[i] = lo[i].min(n.pos_m[i]);
                hi[i] = hi[i].max(n.pos_m[i]);
            }
        }
    }
    if !any {
        return None;
    }
    let m = 0.08;
    Some((
        [lo[0] + m, lo[1] + m, lo[2]],
        [hi[0] - m, hi[1] - m, hi[2] + 0.05],
    ))
}

/// The surface a contact really strikes. The nearest lattice node can sit on a top panel's
/// edge (hood, trunk lid, roof) while the hit is horizontal — a frontal into a wall at the
/// bumper / hood seam, whose height moves with the car's pitch. A top panel only rings as a
/// plate when struck from above; struck sideways, the hit lands on the structure in front of
/// or below its edge: the bumper for the hood (front half) and trunk (rear half), the cabin
/// pillars for the roof.
fn struck_surface(
    m: &MaterialMap,
    local: [f32; 3],
    n_local: [f32; 3],
    kind: wp_contracts::MaterialKind,
    region: BodyRegion,
) -> (wp_contracts::MaterialKind, BodyRegion) {
    let horizontal = n_local[2].abs() < 0.5;
    let target = match region {
        BodyRegion::Hood if horizontal && local[1] > 0.0 => Some(BodyRegion::FrontBumper),
        BodyRegion::Trunk if horizontal && local[1] < 0.0 => Some(BodyRegion::RearBumper),
        BodyRegion::Roof if horizontal => Some(BodyRegion::Cabin),
        _ => None,
    };
    target
        .and_then(|t| {
            m.region_centroid(t)
                .and_then(|c| m.at(c))
                .filter(|x| x.1 == t)
        })
        .unwrap_or((kind, region))
}

/// Horn damage 0..1: the worst front-end region (the horn sits behind the grille) or a weak
/// battery.
fn horn_damage(regions: &BTreeMap<BodyRegion, f32>, s: &VehicleState) -> f32 {
    let front = [
        BodyRegion::FrontBumper,
        BodyRegion::Hood,
        BodyRegion::FrontLeftFender,
        BodyRegion::FrontRightFender,
        BodyRegion::FrameFront,
    ]
    .iter()
    .filter_map(|r| regions.get(r).copied())
    .fold(0.0f32, f32::max);
    let battery = s
        .component_health
        .get(&Component::Battery)
        .map(|h| 1.0 - clamp01(*h))
        .unwrap_or(0.0);
    let radiator = s
        .component_health
        .get(&Component::Radiator)
        .map(|h| 0.6 * (1.0 - clamp01(*h)))
        .unwrap_or(0.0);
    clamp01(front.max(battery).max(radiator))
}

/// Interior camera: the listener sits in `own`'s cabin. Other vehicles are heard through the
/// body (low-passed, quieter); the own horn is under the hood (muffled). The own engine and
/// impacts keep their sound (the impact synth gets `interior` and shapes itself).
fn cabin_muffle(own: EntityId, desires: &mut [Desire]) {
    for d in desires.iter_mut() {
        if d.id.entity != own.0 {
            d.lowpass_cap_hz = 2_200.0;
            d.gain *= 0.55;
        } else if d.id.slot == slot::HORN {
            d.lowpass_cap_hz = 1_600.0;
            d.gain *= 0.7;
        } else if slot::is_one_shot(d.id.slot) && d.bus == Bus::World {
            // Recorded impact layers: the cabin takes the top off the air-borne part.
            d.lowpass_cap_hz = 3_500.0;
        }
    }
}

/// Seed for the backend's cosmetic scatter (grains, tinkles), from the one-shot counter so the
/// director's pitch RNG sequence stays as it was.
fn cosmetic_seed(seq: u32) -> u32 {
    seq.wrapping_mul(0x9E37_79B1) >> 8
}
