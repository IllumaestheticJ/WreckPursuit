# Browser perf pass (`lead/browser-perf`)

- **Who:** helper for the integration lead, 2026-10-20 (owner request: browser performance
  pass on the playable testbed before more content lands).
- **Branch:** `lead/browser-perf` from `origin/lead/round12` (weather, recorded audio,
  garage/career, minimap, panels).
- **Touched:** `apps/testbed-web` only (TypeScript + e2e). **No Rust, no `data/`, no
  `wp_contracts`**: simulation results cannot move. Native == wasm on every scenario and the
  goldens are unchanged (gates below).

## How it was measured

`node e2e/frame_profile.mjs [chase|rain] [seconds]` (new; `npm run profile`): headless
Chromium (Playwright, SwiftShader), 1280×720, Port Vesper v2, throttle held, recorded audio
running. Per frame it reports:

- the frame-loop sections (new `src/perf.ts`, `window.__wp.app.prof`): input, wasm sim step,
  vehicle views (panels / lattice skinning), events, traffic + police + signals, weather,
  camera, audio (VoicePool + director), UI (HUD, minimap, chase readout, plots), render submit;
- the wall frame time and what is left of it (SwiftShader raster + compositor wait), plus one
  GPU-synced render (render + 1-pixel `readPixels`);
- draw calls, triangles, wasm↔JS calls (every function of `sim` and the audio bridge),
  allocated bytes per frame (V8 sampling heap profiler including collected objects = GC
  pressure) with the top allocators, frame-time p50 / p90 / max and frames over 250 ms;
- structure: point lights, city tiles drawn, signal heads drawn, material version bumps.

The "before" column is the round-12 base (`05f92f8`) with only the profiler added (same
script, same machine, same session). Headless SwiftShader is a CPU rasterizer: the numbers are
only comparable with each other, not with a real GPU. Each run is one 15 s window; the chase
itself differs run to run (who crashes, how many police / promoted traffic cars), so read
±10 %.

## Results (1280×720, headless SwiftShader, 4 cores)

| City chase | Before | After (default `auto`) | After, `?gfx=high` |
|---|---|---|---|
| **Real-time factor** | **0.25** | **0.99** | 0.42 |
| fps / frame p50 | 1.0 / 1067 ms | 7.4 / 133 ms | 1.7 / 600 ms |
| frames > 250 ms (lose sim time) in 15 s | 14 / 15 | 2 / 111 | 24 / 25 |
| GPU-synced render | 1000 ms | 89 ms | 578 ms |
| raster / compositor wait per frame | 957 ms | 95 ms | 521 ms |
| JS sections per frame (sum) | 45 ms | 41 ms | 80 ms |
| – wasm sim step | 32.6 ms (1 fps → 0.25 s of sim a frame) | 28.8 ms | 43.1 ms |
| – views (panels, lattice) | 3.4 | 1.6 | 3.6 |
| – traffic + police + signals | 0.17 | 0.24 | 0.27 |
| – weather | 0.07 | 0.05 | 0.09 |
| – audio | 0.76 | 0.87 | 1.50 |
| – UI (HUD, minimap, plots) | 1.03 | 1.05 | 1.29 |
| – render submit (CPU) | 6.1 | 8.2 | 30.0 |
| draw calls | 358 | 603 | 745 |
| triangles | 522k | 145k | 245k |
| point lights | 5 | 1 (hidden unless a siren is on) | 1 |
| city tiles drawn / total | 1 mesh per material, all drawn | 97 / 677 | 271 / 677 |
| signal heads drawn / total | 94 / 94 | 20 / 94 | 20 / 94 |
| material version bumps over 3 steady renders | 216 | 0 | 0 |
| wasm↔JS calls per frame | 43.1 | 53.7 (11 cars on screen vs 6) | 46.6 |
| allocated per frame | 938 KB | 608 KB | 1300 KB¹ |

| Rainy-night chase | Before | After (`auto`) | After, `?gfx=high` |
|---|---|---|---|
| **Real-time factor** | **0.22** | **0.99** | 0.45 |
| fps / frame p50 | 0.86 / 1167 ms | 7.0 / 133 ms | 1.8 / 517 ms |
| GPU-synced render | 1118 ms | 109 ms | 559 ms |
| JS sections (sum) | 96 ms | 44 ms | 84 ms |
| – weather | 1.45 ms | 0.37 ms | 0.89 ms |
| – audio | 1.38 | 0.85 | 1.47 |
| draw calls / triangles | 353 / 522k | 521 / 113k | 475 / 99k |
| allocated per frame | 948 KB | 652 KB | 1309 KB¹ |

¹ Per frame, and a frame at 1.7 fps covers 0.6 s of sim (more events, contacts and crashed-
panel re-skins per frame); per sim second it is about a third of "before".

**After merging the newer `lead/round12`** (fleet / LOD traffic and police meshes, races;
same profile, auto quality): chase RTF **0.951** (5.3 fps, p50 183 ms, 755 draw calls, 339k
triangles with ~18 cars drawn as real meshes, 54 wasm calls, 1116 KB/frame incl. shader
compiles of arriving car models), rain RTF **0.992** (5.6 fps, 503 calls, 264k triangles,
632 KB/frame). Structure unchanged: 1 point light, ~100 / 677 city tiles drawn, 20–29 / 94
signal heads, 0 material bumps.

wasm↔JS calls per frame scale with the cars on screen (telemetry, panel states and lattice
nodes per car); fixed per-frame calls went 7 → 2 for `world_info`. JSON per frame: the events
JSON is no longer parsed and re-stringified for the audio director (passed through), police
status / run result / audio frame stay JSON (small; see requests).

## What was slow, and the fixes (biggest first)

1. **The city was one mesh per material spanning the whole 4 km map** (370k triangles, never
   frustum-culled, no front-to-back sort). `src/render/city_tiles.ts` splits each greybox mesh
   into 320 m tiles at load (same material objects, names `<mesh>#<i>`, own bounding spheres),
   so Three culls what is off-screen and sorts the rest; tiles past a draw distance are hidden
   (fully fogged distance in fog / rain; `?draw=<m>` overrides).
2. **One point light per police car** (5 in the chase), all shading every lit pixel even at
   intensity 0, and a changing count recompiled every lit shader when a unit spawned. Now one
   shared siren light (`SceneView.setSiren`) on the siren car nearest the camera, hidden when
   no siren is on (≈ −45 % GPU time on its own).
3. **Car glass re-resolved its shader program every frame**: transparent + DoubleSide makes
   Three draw back then front faces and flip `side`, bumping `material.version` (216 bumps per
   3 frames, `getParameters` 220 KB/frame). Glass now uses `forceSinglePass` (one draw per
   part).
4. **Signals:** every head in the city was instanced with culling off (~110k triangles of
   lamps and glow). Only heads within 450 m of the camera are packed into the same 8 instanced
   meshes (re-picked when the camera moves 20 m). `drawnStates()` reports 255 for heads not
   drawn; `e2e/run.mjs` compares only drawn heads.
5. **Panels re-skinned every part of every car every frame** and re-uploaded every buffer even
   with nothing dented. Skipped when the nodes array and panel states are unchanged
   (`PanelBody.reskins` counts the real ones); per-call maps / vectors reused.
6. **Graphics quality** (`src/render/quality.ts`, HUD line "gfx …"): `?gfx=auto|high|low`,
   default auto. On a hardware GPU auto = high (MSAA, native resolution, everything as before)
   plus a governor that lowers the resolution (down to 0.6×) only while frames stay over
   40 ms. On a **software rasterizer** (SwiftShader / llvmpipe: no usable GPU, e.g. headless
   or a blocklisted driver) auto = low: no MSAA, 0.5× resolution (governor down to 0.35×),
   900 m draw distance, 30 % of the rain streaks, and **no CSS backdrop blur** behind UI kit
   panels (re-blurring the changing 3D view cost ~90 ms a frame there). Without this profile
   the headless chase reaches 0.42 real time; with it 0.99.
7. **Allocations and DOM in hot paths:** vehicle views (quaternions, arrow vectors, wheel
   part lookups per frame), contact markers (a new group + sphere + material + arrow per
   contact; now a pool of 64 sharing one geometry), telemetry plots (object per sample → typed
   ring buffer, redraw ≤ 20 Hz), weather HUD (innerHTML every frame → on change; fog colours),
   audio listener vectors, `world_info()` 7× → 1× per frame, telemetry fetched only for the
   focus car unless the force-arrow layer is on.
8. **Throttles:** minimap redraw 30 Hz while the full map is closed (`src/ui/map.ts`;
   every frame while it is open), telemetry plots 20 Hz.
9. **Audio:** VoicePool keeps finished voices' gain → lowpass → panner chains and reuses them
   (≈ 15 voices/s in the chase, 3 nodes each); a voice replaced on the same key was released
   but never disconnected (stayed in the graph) — now disconnected once faded.

Not changed (and why):
- **wasm sim step** (~0.9 ms per tick in the chase incl. police + traffic + Rapier): the
  per-tick replay `state_hash` is ~11 % of it, but a recording covers the session from tick 0,
  so it cannot be skipped, and a cheaper hash needs a hash-spec bump + re-bless (not allowed
  here; still the open request in `playtest-feel.md`).
- **Traffic instancing** was already one InstancedMesh (0.2 ms); kept, per-frame `Matrix4` /
  `Vector3` allocations are small.
- **Materials, lights other than the siren, post** — machine S's restyle (see below).
- `preserveDrawingBuffer` stays on (screenshots); `checkShaderErrors` stays on (S writes
  shaders).

## Perf e2e check

`node e2e/perf_check.mjs` (`npm run e2e:perf`; also step 7 of `e2e/run.mjs` = `just web-e2e`,
`PERF=0` skips it) runs the profile on both chases and fails on: real-time factor < 0.75 (target
0.95 printed), > 12 frames over 250 ms in 12 s, > 1600 draw calls, > 700k triangles, > 120
wasm calls, > 2400 KB allocated per frame, more than one point light, an untiled city, all
signal heads drawn, or any material version bump in steady frames. Thresholds are ≈ 2× the
measured values so a slow CI box does not flake; the structural ones are machine-independent.
Before the round-12 merge: chase 0.989, rain 0.982; after it see the gates section.

## For machine S (ws/art, low-poly dark restyle) — merge notes

- No material, colour, light-rig or post code was changed except: panel **glass** gets
  `forceSinglePass: true` (keep it on any transparent DoubleSide material), and per-car siren
  `PointLight`s are gone → one `SceneView.sirenLight`. Please don't add per-object point
  lights; use emissive + your bloom (each point light costs every lit pixel).
- City meshes are replaced by tiles after load: `onCityLoaded(root)` gets the same root, the
  tiles share the **same material objects and names** (`mesh.name` = `<original>#<i>`), so
  traversing by material name works as before. Assigning new materials per tile is fine;
  keep one material per original mesh to keep draw calls flat.
- Distance fog: tile culling reads `scene.fog` (`FogExp2` density / `Fog.far`). If you fog in
  a custom shader, also set `scene.fog` (or `SceneView.drawDistanceM`) so far tiles are culled.
- Bloom / any render target: size it from `renderer.getDrawingBufferSize()` (the pixel ratio
  changes at run time in auto mode) and skip MSAA targets when `scene.quality.antialias` is
  false. The `low` profile is a good place to drop bloom if it costs.
- Signals: lamps/glow are instanced and only near heads are packed; restyle the materials in
  `signals_view.ts` freely, keep the packing.
- `html[data-gfx]` holds the mode; the low profile disables `backdrop-filter` page-wide.

## Changes outside my area
None outside `apps/testbed-web` (+ this doc). `e2e/run.mjs` signals checks now count drawn
heads only.

## Requests
- **Lead / J (audio, optional):** `audio_frame` returns JSON every frame (commands + stats);
  a typed-array command stream would remove the last per-frame JSON parse (~0.3 ms).
- **Lead (hash spec v4, optional):** unchanged from `playtest-feel.md` — the per-tick replay
  hash is the largest removable sim cost in the browser.
- **R (panels):** a crashed car with hanging panels re-skins every frame (legit), allocating
  ~150 KB/frame in `fitFrame` / fold heights; scratch matrices would halve it.

## Gates (2026-10-20, cloud container, after merging `origin/lead/round12` @ 11cbf48)
- `cargo fmt --check`, `cargo clippy --workspace -- -D warnings`: pass.
- `cargo build --target wasm32-unknown-unknown -p wp_wasm`: pass.
- `tools/wasm-determinism/compare.sh`: **native == wasm on all 61 scenarios**.
- `tsc --noEmit`: pass. `just web-e2e` (`e2e/run.mjs`, incl. weather and the new perf check):
  **73 / 73 pass**; perf check: chase RTF 0.918 in that run (0.951 in the standalone profile;
  ≥ 0.75 floor, the 0.95 target is borderline on the merged round-12 content with ~18 meshed
  cars), rain 0.979.
- `cargo test --workspace`: everything passes **except `simcli goldens_match`** (police
  sideswipe night, the four race picks, five weather scenarios, …). This branch has **no diff to
  `origin/lead/round12` in `crates/`, `apps/simcli/`, `data/`, `tools/` or `Cargo.*`**, so the
  failure is on `lead/round12` itself (goldens not re-blessed after its own merges); nothing in
  this pass can move a hash. Not re-blessed here (owner rule: goldens must not move from this
  branch). Lead: re-bless on round 12 with a `[bless]` commit after checking the moves.
