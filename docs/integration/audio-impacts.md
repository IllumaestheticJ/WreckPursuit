# Audio impacts & horns integration notes (owner row 74)

- **Workstream / prompt:** lead helper — "more accurate, harder-hitting impact sounds and better horns"
- **Branch:** `lead/audio-impacts` (from `origin/lead/round12`)
- **Folders:** `crates/wp_audio/src/impact.rs` (new), `apps/testbed-web/src/audio/impacts/*` (new),
  `apps/testbed-web/src/audio/horn.ts` (new), minimal hooks listed below
- **Contract version built against:** 0.3.0 (no contract change)
- **Coordination:** machine J is building the physically modelled engine synth on `ws/audio`
  (`engine_worklet.ts`) at the same time. No engine code was touched: not `synth.ts`, not
  `engine.rs`, not the engine instrument / engine-group code in `voices.ts`, not the engine packs.

## What it does

Impacts are now driven by the physics on top of the CC0 recordings (which still play):

| Layer | Driven by | Where |
|---|---|---|
| **Modal body panel** (≤ 6 plate modes of the struck region; door / hood / roof / wing / trunk / bumper plastic / glass / structure) | region from the lattice, panel damage from `RegionDamaged` (dented: shorter ring, mode scatter, asymmetric buzz + rattle chop), excitation strength from `½·m_eff·v²` (or the tick's impulse), brightness / contact time from closing speed, car mass (panel size) | `impact::body_params` → `impacts/body.ts` |
| **Chassis thump** (pitch-dropping 30–72 Hz) | strength above 0.35, pitch from vehicle mass | same voice |
| **Bass transient + sidechain duck** of Engine / Police / Music | strength above 0.55 (≈ 25 km/h into a wall), depth × audibility at the listener | same voice + `AudioMixer.sidechain` |
| **Granular crunch** of the recorded metal crunches | the lattice's **plastic deformation power**, every frame (`DamageModel::total_plastic_energy_j` via `AudioDirector::set_plastic_energy`); estimated from the collision energy when not measured | looped voice `impact/crunch` (slot 0x1A2) → `impacts/crunch.ts` |
| **Glass**: crack onset, shatter burst, tinkling debris tail (tempered > laminated) | `GlassShattered` (or inferred glass) | `impact::glass_params` → `body.ts` (`kind = 1`) |
| **Friction scrape**: stick-slip pulse train into panel resonances, grinding band, sparks (metal on the world) | sliding speed (stick-slip rate `freq_hz`), contact impulse (`force`), material | scrape voices' `synth` / `freq_hz` → `impacts/scrape.ts` (recorded loop underneath) |
| **Horns**: two detuned electromagnetic disc horns (420 + 500 Hz, buzzy asymmetric waveform, formants, 12 ms attack, settle from flat, vibrato); air horns for trucks (220/277/330 Hz) and police (311/370 Hz) | held button (unchanged hold/release path), `load` = front-end damage / weak battery → weaker, chatter, one horn sags flat; police units with a standard horn get `horn/police` | `horn.ts`; `horn_damage` in the director |
| **Interior perspective** | listener inside the focus car's cabin box (lattice `Cabin` + `Roof` nodes) | other vehicles low-passed 2.2 kHz and −5 dB, own horn muffled 1.6 kHz, own recorded impact layers capped 3.5 kHz, impact synth `interior = 1` (air-borne part low-passed, thump ×1.5) |

Distance attenuation, Doppler (folded into `pitch` / `freq_hz`) and air absorption come from the
existing director / mixer for every new voice. Occlusion by buildings is still not modelled
(no geometry reaches audio; see Known gaps).

**Setting:** audio panel → **impacts & horns: synth / recorded** (default synth, persisted).
`recorded` = the previous behaviour (recordings only, recorded horns). The procedural pack
(`world_proc`) always uses the horn models.

**CPU:** native nodes only (oscillators, biquads, wave shapers, buffer sources), no worklet. An
impact voice is ≤ 11 scheduled sources that stop themselves; the crunch is ≤ 24 overlapping
grains; voices go through the director's normal culling and voice limits (`impact/body` High,
glass Critical, crunch High).

## Exposes

- `wp_audio::impact` (new, pub): `ImpactSynthConfig` (`audio_config.json` → optional
  `impact_synth` block, defaults in code), `PanelClass`, `Hit`, `strength`, `reduced_mass`,
  `body_params`, `glass_params`, `CrunchState`, `scrape_rate_hz`, `scrape_force`,
  `det_log2` / `det_exp2` / `det_pow` (deterministic, arithmetic only).
- `AudioDirector::set_plastic_energy(entity, total_plastic_j)` (call before `update`, once per
  frame per vehicle), `AudioDirector::interior() -> Option<EntityId>`.
- `slot::CRUNCH`; cue names `cues::IMPACT_BODY` (`impact/body`), `cues::IMPACT_CRUNCH`
  (`impact/crunch`), `cues::HORN_POLICE` (`horn/police`) — all optional in a World pack.
- `KNOWN_SYNTHS` += `impact_body`, `crunch`, `friction`, `disc_horn`, `air_horn`.
- `Desire::lowpass_cap_hz` (mixer: the voice's low-pass is limited to it; 0 = none).
- Browser: `VoicePool.impacts: ImpactSynths` (`enabled`, `log`, `built`), `AudioMixer.sidechain`,
  `AudioMixer.out`, `AudioSystem.setImpacts(mode)`, `.impactLog()`, `.contactFrames`.

## Hooks in existing files (minimal; for whoever merges `ws/audio`)

| File | Change |
|---|---|
| `apps/testbed-web/src/audio/voices.ts` | import `ImpactSynths`; `BusNodes.side` gain between `user` and `duck` + `AudioMixer.sidechain()`; master: compressor → **soft clip** (identity < 0.7, tanh knee to 0.99) → destination, `AudioMixer.out`; `VoicePool.impacts` + `decodedBuffers()`; in `play()`: `this.impacts.build(...)` before the strip (`"skip"` → nothing; a synth → used instead of sample / `makeSynth`); `applyParams`: `everyFrame` synths get `update` every frame. Engine instrument code untouched. |
| `apps/testbed-web/src/audio/index.ts` | `setImpacts`, `impactLog`, `contactFrames` (stamped when a frame's events hold a `Contact`), the setting applied on start. |
| `apps/testbed-web/src/audio/panel.ts` | `impacts` setting + select "impacts & horns". |
| `crates/wp_audio/src/director.rs` | `CarAudio.{horn_police, mass_kg, cabin}`, `WorldCues.{body, crunch}`, `VehicleRuntime.{region_damage, crunch}`, interior detection, `body_hits` / `glass_body` / `crunch_desire`, horn `load` + police air horn, scrape friction params, `cabin_muffle`, helpers at the end of the file. |
| `crates/wp_audio/src/{collision,data,mixer,types,lib}.rs` | `PairState.{slide_mps, force, frame_force, world}`; `AudioConfig.impact_synth` (serde default), cue names, `KNOWN_SYNTHS`; `Desire::lowpass_cap_hz`; `slot::CRUNCH`; `pub mod impact`. |
| `crates/wp_wasm/src/audio.rs` | one line: `d.set_plastic_energy(*id, v.damage.total_plastic_energy_j())`. |
| `data/audio/packs/world_proc.json` | `horn/standard` → `disc_horn`, `horn/truck` → `air_horn` (3 trumpets), new `horn/police`, `impact/body`, `impact/crunch`. |
| `data/audio/packs/world_rec.json` | regenerated by `tools/audio_packs/write_packs.py` (re-run is byte-stable; only the world packs changed): inherits the new cues, `horn/police` recorded fallback = the truck horn recording. |
| `data/audio/cars/sentinel_{cruiser,wolfhound}.json` | `horn_cue` → `horn/police`. |
| `crates/wp_audio/Cargo.toml` | dev-dependency `simcli` (the trace runs real scenarios). |

New files: `crates/wp_audio/src/impact.rs`, `crates/wp_audio/tests/impacts.rs`,
`crates/wp_audio/examples/impact_trace.rs`, `apps/testbed-web/src/audio/impacts/{index,body,crunch,scrape,util}.ts`
+ `README.md`, `apps/testbed-web/src/audio/horn.ts`, `apps/testbed-web/e2e/audio_impacts.mjs`,
`docs/media/audio/impacts/*.wav` + `README.md`.

## Validation

`cargo run --release -p wp_audio --example impact_trace -- t.json` runs **real crashes**
(simcli scenarios: Rapier contacts, `wp_damage` lattice, measured plastic energy) through the
director at 60 Hz; `node apps/testbed-web/e2e/audio_impacts.mjs [t.json]` plays them through
the real `VoicePool` / `AudioMixer` in an `OfflineAudioContext`, writes the WAVs and checks.
Results on this branch (cloud container, headless Chromium):

| Check | Result |
|---|---|
| Contact event → impact audible (sim time → rendered World bus), frontal 20 / 50 / 90 km/h, interior, wall scrape | 5.5 / 11.7 / 9.7 / 9.7 / 12.3 ms (≤ 30) — the director runs at 60 Hz, so ≤ one frame + attack |
| Live testbed (wasm, barrier crash): frame that drained the contact → impact voice built / scheduled | 19.3 ms (incl. the wasm director call in software GL) / +0.0 ms; device latency on top (42 ms headless) |
| Hit RMS 20 / 50 / 90 km/h (World bus, 200 ms) | −16.5 / −12.2 / −9.5 dB (recordings alone: −26.3 / −17.9 / −17.4) |
| Bass < 120 Hz 20 / 50 / 90 km/h | −19.1 / −14.7 / −12.4 dB (recordings alone: −30.9 / −20.5 / −21.1) |
| Synthesis over recordings alone, peak / bass | 50 km/h +3.6 / +5.7 dB, 90 km/h +4.6 / +8.6 dB |
| Sidechain (engine bus during the 90 km/h hit vs just before) | −2.3 dB vs +5.6 dB without synthesis (−7.9 dB relative); 20 km/h barely ducks |
| Crunch / debris 100–400 ms after the hit | 20 km/h −25.7 dB, 90 km/h −18.0 dB; the first crush crunches ~0.4 s at 20 km/h and ~0.9 s at 90 km/h, following the measured plastic energy (secondary hits crunch again) |
| Interior vs exterior (50 km/h), bass − treble | 20.7 vs 17.7 dB |
| Glass: onset / tail > 4 kHz | 0 ms each; tempered rear 1.39 s, laminated windshield 0.83 s; the 90 km/h crash really breaks the windshield (`GlassShattered` from `wp_damage`) |
| Wall scrape (70 km/h, 2.9 s of contact, 183 contact events) | continuous (worst 50 ms window −9 dB from the median), sparks present |
| Horns held 2.0 s (modelled): disc pair / damaged / truck air / police air | sound 2.020 / 2.020 / 2.015 / 2.010 s, onset 5 / 5 / 10 / 15 ms, worst 20 ms dip −0.5 / −3.3 (chatter) / −0.1 / −0.1 dB, quiet 45–50 ms after release |
| Horns, recorded fallback setting | 2.020–2.025 s, release 45 ms (the row-58 behaviour holds) |
| Damaged front end | −9.5 dB and rattly (10 ms envelope roughness 2.45 vs 0.31 dB) |
| Modelled vs recorded horn level | 0.0 dB (disc), −0.6 dB (truck); recorded vs procedural mode identical |
| Clipping | master ≤ −0.1 dBFS on every render (soft clip); no NaN |
| `e2e/audio_roster.mjs` horn checks (row 58) | pass with the modelled horns (2.02 s, onset 0–10 ms, release 50 ms) |

WAVs: `docs/media/audio/impacts/` (see its README).

**Gate run on this branch (2026-10-20):** `cargo fmt --check` ✓, `cargo clippy --workspace
--all-targets -- -D warnings` ✓, `cargo build --target wasm32-unknown-unknown -p wp_wasm` ✓ (and
`npm run wasm`), `tsc` ✓, `e2e/audio_impacts.mjs` ✓ (56 checks incl. live), `e2e/audio.mjs` ✓,
`e2e/audio_samples.mjs` ✓, `e2e/audio_roster.mjs` ✓ (row-58 horn hold checks pass with the
modelled horns), `e2e/race.mjs` ✓. Not green, and not from this branch:
- `cargo test --workspace`: everything passes except simcli `goldens_match` (18 scenarios, as
  already reported on `lead/round12` in `docs/integration/audio.md`), `wp_opponent` race
  `live_police_chase_ends_in_escape_or_capture_without_stuck_cars`, and `wp_race` race_picker
  `loop9_circuit` / `lantern_checkpoint_draws_the_police`. None of these crates depend on
  `wp_audio` or read audio data; the same three tests fail identically on an untouched
  `origin/lead/round12` worktree.
- `e2e/run.mjs` (part of `just web-e2e`): every check passes except `perf_check.mjs`'s
  software-GL frame-rate check (2.0–2.3 fps vs real-time factor ≥ 0.75, also on an idle machine).
  That check never presses a key, so the AudioContext never starts and none of this work runs.

**Round 13 re-base (2026-10-20, after A's vehicle / ride / countersteer work merged):** four
checks failed on `lead/round12` — 20 km/h louder than expected, bass and crunch ordering, wall
sparks. Cause: the car's new pitch moved the 20 km/h barrier contact a few cm up, the nearest
lattice node became the hood's front edge, and the director rang the whole hood plate (and the
recordings switched to metal dents). Fix in the mapping, not the thresholds:
`director::struck_surface` — a top panel (hood, trunk, roof) only rings as a plate when struck
from above (|n_z| ≥ 0.5 in chassis space); a horizontal hit on its edge strikes the bumper (hood
front half / trunk rear half) or the cabin pillars (roof), for the recorded layers too (test
`a_horizontal_hit_on_the_hood_edge_strikes_the_bumper`). 20 / 50 / 90 km/h hit RMS is now
−16.7 / −12.2 / −9.5 dB again. The wall-scrape spark check was an absolute level against the
body that depended on how far the car slides from the ear (round 13: 71 contact events instead of
183); it now checks that the synthesis adds the > 5 kHz band over the recordings alone
(−60.5 vs −67.4 dB) and the spark gain follows the scrape level linearly. WAVs regenerated.

## Needs

| From | What | Why | Workaround now |
|---|---|---|---|
| Lead (`wp_sim`) | A-4 (sustained contacts) | car-to-car sideswipes at similar speeds emit only a few events above 50 N·s; scrapes between cars are short | the wall scrape is continuous (183 events in 2.9 s); car-to-car scrapes hold for `scrape_release_s` after each event |
| Lead / B | A-1 (material / region on `SimEvent::Contact`) | the struck region comes from the nearest lattice node (a frontal into a barrier resolves to the bumper; the director lifts it to the steel behind the cover for hard hits) | `MaterialMap` lookup |
| B (03) | per-hit plastic energy (or per-region deltas in an event) | the crunch follows the car's total plastic energy, so two simultaneous hits on one car share one crunch | one crunch loop per vehicle (physically: one body folding) |

## Contract change requests
None new (A-1 and A-4 in `docs/integration/audio.md` would sharpen this work).

## Perf numbers
| Measurement | Machine | Native | Browser | Budget |
|---|---|---|---|---|
| `AudioDirector::update`, 24 cars, 8 contacts/frame (`tests/perf.rs`, release) | cloud container | 0.110 ms/frame (was 0.089 before this work) | — | 0.5 ms native |
| Building one `impact/body` voice (main thread) | headless Chromium | — | mean 1.4 ms, max 3.8 ms (16 voices, while also rendering) | one-off per hit |
| Offline render of all cases, every bus (engines, tyres, impacts, crunch, scrapes, horns) | headless Chromium | — | 9 % of real time | — |
| Voices per hit | — | — | +1 `impact/body` per struck car (≤ 11 sources, self-stopping) + 1 crunch loop per crushed car (≤ 24 grains); director voice limits apply | `max_voices` 48 |

## Known gaps
- No occlusion by buildings / tunnels (no geometry reaches audio); distance, Doppler and air
  absorption only.
- Interior detection is the focus car's cabin box; hood / bumper cameras count as exterior.
- The modal bank is per region class, not per car model; a truck's panels only scale with mass.
- Headless renders only: tuned by measurement and spectrograms, not yet by ear on speakers.
- Device output latency (`AudioContext.baseLatency + outputLatency`, ~42 ms in headless
  Chromium) adds to every sound, impact or not; the ≤ 30 ms check covers contact event → voice.

## Lead feedback
(Written by the integration lead.)
