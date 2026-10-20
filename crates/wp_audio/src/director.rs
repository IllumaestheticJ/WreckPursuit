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
    siren: [CueRef; 4],
    world: WorldCues,
    materials: MaterialMap,
    static_load_n: f32,
    /// Loaded wheel radii FL, FR, RL, RR, m (wheelspin slip speed).
    wheel_radius_m: [f32; 4],
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
}

const DEFAULT_WORLD_PACK: &str = "world_proc";
const DEFAULT_SIREN_PACK: &str = "siren_proc";

impl AudioDirector {
    /// New director over `lib` with a seed for the (purely cosmetic) pitch variation.
    pub fn new(lib: AudioLibrary, seed: u64) -> Self {
        let mut cues = CueTable::from_library(&lib);
        let default_world = WorldCues::new(&mut cues, DEFAULT_WORLD_PACK);
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
        }
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
            wheel_radius_m: car.def.wheels.each_ref().map(|w| w.radius_m),
            layers,
            def,
        };
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

        let mut desires: Vec<Desire> = Vec::new();
        let mut unregistered = 0;
        for v in &vehicles {
            if !self.cars.contains_key(v.car_id) {
                unregistered += 1;
                continue;
            }
            self.vehicle_desires(
                v,
                frame.focus == Some(v.entity),
                frame.listener.pos,
                tick,
                dt,
                &mut desires,
            );
        }
        self.event_desires(frame.events, &by_id, tick, &mut desires);
        self.scrape_desires(&by_id, tick, &mut desires);

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

    fn vehicle_desires(
        &mut self,
        v: &VehicleAudioInput,
        focus: bool,
        listener_pos: [f32; 3],
        tick: u64,
        dt: f32,
        out: &mut Vec<Desire>,
    ) {
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
            let eo = update_engine(
                &mut rt.engine,
                &cfg.engine,
                &car.def.engine,
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
        // Horn.
        if v.horn {
            let mut d = Desire::new(
                VoiceId::new(e, slot::HORN),
                Bus::World,
                car.horn,
                up(Priority::High),
                pos,
                vel,
            );
            d.gain = cfg.sirens.horn_gain;
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
                    let side = |v: Option<&VehicleAudioInput>| {
                        v.and_then(|v| {
                            let car = &self.cars[v.car_id];
                            let local = world_to_local(v.state.pos, v.state.rot, *point);
                            car.materials.at(local).map(|(k, r)| (v.entity, k, r))
                        })
                    };
                    let sides: Vec<_> = [side(va), side(vb)].into_iter().flatten().collect();
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
                        }
                        let g = impact_gain(&mix, *impulse_ns, *rel_speed);
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
                    if is_panel(*region) {
                        if let Some(r) = self.vehicles.get_mut(vehicle) {
                            r.panel_damage = r.panel_damage.max(clamp01(*severity));
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
        let id = self.next_one_shot(vehicle.0);
        let mut d = Desire::new(
            id,
            Bus::World,
            cue,
            Priority::Critical,
            point,
            v.state.lin_vel,
        );
        d.gain = self.lib.config.impacts.impact_gain;
        d.pitch = self.rng.range_f32(0.92, 1.08);
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
            }
            p.frame_scrape = 0.0;
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
