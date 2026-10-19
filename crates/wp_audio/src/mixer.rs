//! Voice management: every frame the director lists the voices it *wants* ([`Desire`]s); the
//! [`VoiceMixer`] spatialises them, culls to the global and per-bus voice limits by
//! `priority weight × audible gain`, and diffs against what is already playing to produce
//! `Play` / `Set` / `Stop` commands. Sorting is total and tie-broken by [`VoiceId`], so the
//! command stream is deterministic.

use std::collections::BTreeMap;

use wp_contracts::PHYSICS_DT;

use crate::data::{AudioConfig, AudioLibrary, CueSource};
use crate::spatial::spatialise;
use crate::types::{AudioCommand, Bus, Listener, Priority, VoiceId, VoiceParams};

/// Interned `(pack, cue)` pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CueRef(pub u32);

/// What the mixer knows about a cue.
#[derive(Debug, Clone, PartialEq)]
pub struct CueInfo {
    /// Pack id.
    pub pack: String,
    /// Cue name.
    pub cue: String,
    /// Cue gain from the pack.
    pub gain: f32,
    /// One-shot length, s (looped cues: 0).
    pub duration_s: f32,
    /// Whether the cue is a loop.
    pub looped: bool,
    /// Recorded speed for surface loops, m/s.
    pub root_speed_mps: Option<f32>,
    /// Whether the pack actually defines it (false = placeholder for a missing cue).
    pub defined: bool,
    /// The cue's procedural synth params (empty for samples).
    pub synth_params: BTreeMap<String, f32>,
}

/// Interning table for every cue of every pack.
#[derive(Debug, Clone, Default)]
pub struct CueTable {
    infos: Vec<CueInfo>,
    index: BTreeMap<(String, String), CueRef>,
}

impl CueTable {
    /// Intern every cue of every pack in `lib`.
    pub fn from_library(lib: &AudioLibrary) -> Self {
        let mut t = Self::default();
        for p in lib.packs.values() {
            for (name, c) in &p.cues {
                let synth_params = match &c.source {
                    CueSource::Synth { params, .. } => params.clone(),
                    CueSource::Samples { .. } => BTreeMap::new(),
                };
                t.push(CueInfo {
                    pack: p.id.clone(),
                    cue: name.clone(),
                    gain: c.gain,
                    duration_s: if c.looped {
                        0.0
                    } else {
                        c.duration_s.unwrap_or(1.0)
                    },
                    looped: c.looped,
                    root_speed_mps: c.root_speed_mps,
                    defined: true,
                    synth_params,
                });
            }
        }
        t
    }

    fn push(&mut self, info: CueInfo) -> CueRef {
        let r = CueRef(self.infos.len() as u32);
        self.index.insert((info.pack.clone(), info.cue.clone()), r);
        self.infos.push(info);
        r
    }

    /// Look up (or create a placeholder for) `pack`/`cue`. Placeholders keep the game
    /// audible when data is incomplete; [`AudioLibrary::validate`] reports them.
    pub fn get(&mut self, pack: &str, cue: &str, looped_hint: bool) -> CueRef {
        if let Some(r) = self.index.get(&(pack.to_string(), cue.to_string())) {
            return *r;
        }
        self.push(CueInfo {
            pack: pack.to_string(),
            cue: cue.to_string(),
            gain: 1.0,
            duration_s: if looped_hint { 0.0 } else { 1.0 },
            looped: looped_hint,
            root_speed_mps: None,
            defined: false,
            synth_params: BTreeMap::new(),
        })
    }

    /// Info for an interned cue.
    pub fn info(&self, r: CueRef) -> &CueInfo {
        &self.infos[r.0 as usize]
    }
}

/// A voice the director wants this frame (before spatialisation and culling).
#[derive(Debug, Clone, PartialEq)]
pub struct Desire {
    /// Voice identity.
    pub id: VoiceId,
    /// Mixer bus.
    pub bus: Bus,
    /// Cue.
    pub cue: CueRef,
    /// Priority class.
    pub priority: Priority,
    /// Gain before distance attenuation and cue gain.
    pub gain: f32,
    /// Pitch before Doppler.
    pub pitch: f32,
    /// Synth fundamental before Doppler, Hz (0 = n/a).
    pub freq_hz: f32,
    /// Synth load / intensity 0..=1.
    pub load: f32,
    /// Periodic phase 0..1.
    pub phase: f32,
    /// World position, m.
    pub pos: [f32; 3],
    /// World velocity, m/s.
    pub vel: [f32; 3],
    /// Extra synth params sent with `Play` (per-car engine timbre, siren cadence).
    pub synth: Option<BTreeMap<String, f32>>,
}

impl Desire {
    /// A desire with neutral parameters at `pos`.
    pub fn new(
        id: VoiceId,
        bus: Bus,
        cue: CueRef,
        priority: Priority,
        pos: [f32; 3],
        vel: [f32; 3],
    ) -> Self {
        Self {
            id,
            bus,
            cue,
            priority,
            gain: 1.0,
            pitch: 1.0,
            freq_hz: 0.0,
            load: 0.0,
            phase: 0.0,
            pos,
            vel,
            synth: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct InFlight {
    bus: Bus,
    end_tick: u64,
}

/// Running-voice bookkeeping and the culling/diff step.
#[derive(Debug, Clone, Default)]
pub struct VoiceMixer {
    /// Looped voices currently playing.
    active: BTreeMap<VoiceId, Bus>,
    /// One-shots still sounding (they hold a voice until their duration ends).
    in_flight: BTreeMap<VoiceId, InFlight>,
}

/// Snapshot of mixer occupancy (debug HUD and tests).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MixerStats {
    /// Playing looped voices.
    pub looped: usize,
    /// Sounding one-shots.
    pub one_shots: usize,
    /// Voices per bus (looped + one-shots).
    pub per_bus: BTreeMap<Bus, usize>,
    /// Desires culled in the last frame (inaudible or over a limit).
    pub culled: usize,
}

impl VoiceMixer {
    /// Spatialise, cull and diff. `tick` is the simulation tick of this frame.
    pub fn mix(
        &mut self,
        cfg: &AudioConfig,
        cues: &CueTable,
        listener: &Listener,
        tick: u64,
        desires: Vec<Desire>,
        out: &mut Vec<AudioCommand>,
    ) -> MixerStats {
        self.in_flight.retain(|_, f| f.end_tick > tick);

        // Budget left after the one-shots that are still sounding.
        let mut total_left = cfg.max_voices.saturating_sub(self.in_flight.len());
        let mut bus_left: BTreeMap<Bus, usize> = BTreeMap::new();
        for bus in Bus::ALL {
            if let Some(max) = cfg.bus_max_voices.get(&bus) {
                let used = self.in_flight.values().filter(|f| f.bus == bus).count();
                bus_left.insert(bus, max.saturating_sub(used));
            }
        }

        let mut cands: Vec<(f32, Desire, VoiceParams)> = Vec::with_capacity(desires.len());
        let mut culled = 0;
        for d in desires {
            let info = cues.info(d.cue);
            let sp = spatialise(&cfg.spatial, d.pos, d.vel, listener.pos, listener.vel);
            let gain = d.gain * info.gain * sp.gain;
            // NaN-safe: a NaN gain is culled too.
            if gain.is_nan() || gain < cfg.min_audible_gain {
                culled += 1;
                continue;
            }
            let params = VoiceParams {
                gain,
                pitch: d.pitch * sp.doppler,
                freq_hz: d.freq_hz * sp.doppler,
                load: d.load,
                phase: d.phase,
                lowpass_hz: sp.lowpass_hz,
                doppler: sp.doppler,
                pos: d.pos,
            };
            let keep = if self.active.contains_key(&d.id) {
                cfg.keep_bonus
            } else {
                1.0
            };
            let score = cfg.priority_weights[d.priority.index()] * gain * keep;
            cands.push((score, d, params));
        }
        cands.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.id.cmp(&b.1.id)));

        let mut chosen: BTreeMap<VoiceId, (Desire, VoiceParams)> = BTreeMap::new();
        let mut new_one_shots: Vec<(Desire, VoiceParams)> = Vec::new();
        for (_, d, p) in cands {
            if total_left == 0 {
                culled += 1;
                continue;
            }
            if let Some(left) = bus_left.get_mut(&d.bus) {
                if *left == 0 {
                    culled += 1;
                    continue;
                }
                *left -= 1;
            }
            total_left -= 1;
            if cues.info(d.cue).looped {
                chosen.insert(d.id, (d, p));
            } else {
                new_one_shots.push((d, p));
            }
        }

        // Stops first (frees backend voices), then plays, then updates; each in id order.
        let stopped: Vec<VoiceId> = self
            .active
            .keys()
            .filter(|id| !chosen.contains_key(id))
            .copied()
            .collect();
        for id in stopped {
            self.active.remove(&id);
            out.push(AudioCommand::Stop {
                voice: id,
                fade_s: cfg.stop_fade_s,
            });
        }
        new_one_shots.sort_by_key(|a| a.0.id);
        let mut sets = Vec::new();
        for (id, (d, p)) in &chosen {
            if self.active.contains_key(id) {
                sets.push(AudioCommand::Set {
                    voice: *id,
                    params: *p,
                });
            } else {
                self.active.insert(*id, d.bus);
                out.push(play(cues, d, *p));
            }
        }
        for (d, p) in &new_one_shots {
            let dur = cues.info(d.cue).duration_s;
            let ticks = (dur / PHYSICS_DT).ceil().max(1.0) as u64;
            self.in_flight.insert(
                d.id,
                InFlight {
                    bus: d.bus,
                    end_tick: tick + ticks,
                },
            );
            out.push(play(cues, d, *p));
        }
        out.extend(sets);

        let mut per_bus: BTreeMap<Bus, usize> = BTreeMap::new();
        for b in self.active.values() {
            *per_bus.entry(*b).or_default() += 1;
        }
        for f in self.in_flight.values() {
            *per_bus.entry(f.bus).or_default() += 1;
        }
        MixerStats {
            looped: self.active.len(),
            one_shots: self.in_flight.len(),
            per_bus,
            culled,
        }
    }

    /// Stop everything (session reset). Pushes a `Stop` for every looped voice.
    pub fn stop_all(&mut self, fade_s: f32, out: &mut Vec<AudioCommand>) {
        for id in self.active.keys() {
            out.push(AudioCommand::Stop { voice: *id, fade_s });
        }
        self.active.clear();
        self.in_flight.clear();
    }

    /// Whether a looped voice is currently playing.
    pub fn is_active(&self, id: VoiceId) -> bool {
        self.active.contains_key(&id)
    }
}

fn play(cues: &CueTable, d: &Desire, params: VoiceParams) -> AudioCommand {
    let info = cues.info(d.cue);
    AudioCommand::Play {
        voice: d.id,
        bus: d.bus,
        pack: info.pack.clone(),
        cue: info.cue.clone(),
        looped: info.looped,
        priority: d.priority,
        params,
        synth: d.synth.clone().unwrap_or_default(),
    }
}
