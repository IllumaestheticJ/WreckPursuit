# Browser perf pass (`lead/browser-perf`)

- **Who:** helper for the integration lead, 2026-10-20 (owner request: browser performance
  pass on the playable testbed before more content lands).
- **Branch:** `lead/browser-perf` from `origin/lead/round12` (weather, recorded audio,
  garage/career, minimap, panels).
- **Touched:** `apps/testbed-web` only (TypeScript + e2e). **No Rust, no `data/`, no
  `wp_contracts`**: simulation results cannot move. Native == wasm on every scenario and the
  goldens are unchanged (gates below).

# Round 12b regression pass (2026-10-20)

- **Why:** after round 12b merged the low-poly dark style (post stack, palette, bloom), crash
  effects, instant crash replay, drawn parked cars, the game menu / HUD layout and touch
  controls, `e2e/perf_check.mjs` failed on `lead/round12`: chase RTF 0.63, rain 0.59, 23 frames
  over 250 ms, 2.5–2.8 MB allocated per frame, an intermittent "6 material version bumps
  (MeshBasicMaterial)"; phone profile 0.96 → 0.71 with 659 draw calls (`touch.md`).
- **Branch:** `lead/browser-perf`, `origin/lead/round12` (c1a98ab) merged in.
- **Touched:** `apps/testbed-web` only (TypeScript + e2e) and this doc. **No Rust, no `data/`,
  no `wp_contracts`, no sim change:** `git diff c1a98ab -- crates data apps/simcli Cargo.lock`
  is empty, so determinism and the goldens cannot move. No feature removed; on a hardware GPU
  ("high" / auto on a GPU) the image is unchanged; the extra savings are on the software-GL and
  touch profiles only (lean distances below).

## Before / after (this container, headless SwiftShader, 4 cores, 12 s windows)

`node e2e/frame_profile.mjs` on the untouched round-12 tree (c1a98ab, same wasm) and on this
branch, interleaved, two runs each (`style=lowpoly`, auto quality). The chase is not identical
run to run (who crashes, how many police / promoted cars), read ±10 %.

| City chase 1280×720 | Before (c1a98ab) | After |
|---|---|---|
| **Real-time factor** | **0.61 / 0.65** | **0.84 / 0.75** |
| fps · frame p50 / p90 | 2.5–2.7 · 317–333 / 583–767 ms | 3.4–3.9 · 233–283 / 350–450 ms |
| frames > 250 ms in 12 s | 25–26 | 17–21 |
| GPU-synced render | 282–300 ms | 155–186 ms |
| JS sections per frame | 146–158 ms (render submit 51–67) | 89–106 ms (render submit 8–10) |
| draw calls / triangles | 742–778 / 443–456 k | 280–297 / 190–196 k |
| allocated per frame (steady + one-off) | 2511–2574 KB (992–1164 steady) | 968–1068 KB (690–757 steady) |
| material version bumps over 3 renders | 0 (6 intermittent) | 0 |

| Rainy-night chase 1280×720 | Before | After |
|---|---|---|
| **Real-time factor** | **0.60 / 0.58** | **0.79 / 0.82** |
| frames > 250 ms | 25 / 25 | 26 / 25 |
| GPU-synced render | 301–409 ms | 176–182 ms |
| draw calls / triangles | 676–701 / 403–413 k | 279–292 / 180–181 k |
| allocated per frame (steady) | 2924–2988 KB (1235–1440) | 1166–1293 KB (787–908) |

| Phone 740×360 touch (chase) | Before | After |
|---|---|---|
| **Real-time factor** | **0.76 / 0.73** | **0.81 / 0.84** |
| frames > 250 ms | 22 / 23 | 14 / 16 |
| **draw calls** / triangles | **678** / 295 k | **263–265** / 172–174 k |
| GPU-synced render · render submit | 250–262 ms · 46–51 ms | 156–166 ms · 7–9 ms |
| allocated per frame (steady) | 2127–2145 KB (1091–1105) | 856–895 KB (626–672) |
| material version bumps | 6 / 6 | 0 |

**Machine calibration (read this before the targets).** This container is slower at raster than
the one the first pass ran on: the **pre-12b build (`4cce4f1`) gets chase 0.85–0.95 and rain
0.89–0.95 here, with 16–22 frames over 250 ms** (it passed at 0.92+ there). Against that, the
12b content now costs about 0.05–0.1 RTF on this box (from ~0.3). The targets of this pass:

| Target | Result here |
|---|---|
| desktop chase / rain ≥ 0.9 RTF | **not met here**: 0.75–0.84 / 0.79–0.83 (pre-12b build: 0.85–0.95) |
| < 12 slow frames | **not met here**: desktop 15–26, phone 14–17 (pre-12b build: 16–22) |
| < 0.2 MB/frame steady-state alloc | **not met**: 0.63–0.9 MB at 3–4 fps (was 1.0–1.4; total 2.5–3.0 → 0.9–1.3 MB) |
| 0 material bumps | **met** (structural, every run) |
| phone ≥ 0.9 RTF | **not met here**: 0.81–0.89 (was 0.71–0.76) |
| phone < 300 draw calls | **met**: 257–281 (was 659–678) |

What still bounds the frame here: the GPU process (SwiftShader rasterizing + compositing the
1280×720 page) runs ~170–200 ms a frame and does not overlap the main thread's JS (~90 ms, of
which the wasm sim is 65–100 ms: 0.25–0.3 s of CPU per wall second, unchanged by design), so the
frame is their sum. Steady allocation at 3–4 fps is a long tail (vertex re-skinning in V8's
Maglev tier, crash-effect wheel pass, Three internals, wasm-bindgen arrays) — see Requests.

## What was slow in 12b, and the fixes (biggest first)

1. **Shader programs compiled in the middle of the chase** (the 0.5–1.3 s frames). The one
   shared siren `PointLight` is hidden until a siren comes on; when the first police car
   switched it on, the point-light count went 0 → 1 and **every lit material compiled a second
   program** (~15 at once). Effect pools, the contact markers and headlight pools also
   compiled on first use. Under SwiftShader a program costs ~200–300 ms and its first query
   (`getProgramParameter`) is a synchronous wait for the GPU process. Fix: `SceneView.prewarm`
   (src/scene.ts), at most once a second, compiles every *new* material (incremental: a small
   scene of stand-ins sharing geometry + material, not `renderer.compile` over thousands of
   city objects) for **both** siren states, for the post stack's HDR target, and queries the
   programs so the link happens there; `prewarmRoots` covers things that start hidden (crash
   effects, the first contact marker, now created up front, a prototype headlight pool).
   Steady state: no program is created after ~8 s of chase; the frame loop has no
   `GetProgramiv` waits in a Chrome trace (was 16–20 ms/frame).
2. **Material churn ("6 version bumps, MeshBasicMaterial")**: the crash-effect skid marks are
   transparent + DoubleSide without `forceSinglePass`, so Three draws back then front faces and
   flips `side`, bumping the version twice a render (only while skid marks exist: the
   intermittent failure). `forceSinglePass` on skids, the race-gate beam and the box stand-in.
3. **Draw calls** (774 → ~290 desktop, 659 → ~264 phone):
   - a full car view was ~37 meshes (one per panel part): `PanelBody` now **batches the body
     parts per material** into one mesh (each part's `pos` is a view into the batch buffer, so
     skinning, debris and `materialCentre` are unchanged; a hidden part's index range is
     collapsed); lenses (their material swaps when broken) and wheel parts (they spin) stay
     separate: ~16 draws per car;
   - **far promoted cars go back to the fleet**: a traffic / police view beyond the fleet's
     full-mesh distance (± 15 m) is hidden and the instanced fleet draws it (same model, livery
     and pose, the existing seamless hand-over); its lattice fetch and re-skin are skipped
     meanwhile. Police 800 m away were drawing 41 meshes;
   - parked cars: **192 m chunks** (were 96 m, 4 draws per proxy chunk), chunks past the city's
     own cull distance (draw distance / fully fogged) not drawn (never within 200 m; e2e
     counts them as `culledFar`).
4. **Triangles** (456 k → ~190 k): the two-box parked car lost its never-seen bottom faces (all
   profiles); on the **lean (software-GL and touch) profiles**: full car meshes within 100 m
   (were 250 / 120), at most 8 full-mesh parked cars (24), parked proxies within 150 m (300),
   parked cars drawn within 450 m on software GL (350 on a phone as before), signal heads within
   250 m (450), street-lamp poles within 300 m (the glowing heads to the draw distance), kerbs /
   lot skirts / corner fills within 250 m (`CITY_DETAIL`, city_tiles.ts). At ≤ 0.5× resolution
   these are a few pixels.
5. **Fill**: the sky dome was drawn first over the whole screen; it now sits on the far plane
   (`gl_Position = p.xyww`) and draws after the opaque scene with the depth test (identical
   image, ~12 ms less per frame here). Software GL: rain streaks 20 % (were 30 %; 30 % cost
   ~25 ms), governor floor 0.3× (0.35×).
6. **Allocations** (2.5–3.0 → 0.9–1.3 MB/frame):
   - each promotion built a whole car view: ~25 geometries, ~45 materials (each a new
     shader-program binding) and every per-part array re-derived from the car JSON. Now
     geometries are cached by size, materials that are the same on every car are shared (6 panel
     materials, nose, spokes, debug markers), per-part arrays and merged indices are cached per
     model, and the debug overlays (force arrows, suspension bars, lattice, stress points,
     skinned body) are built the first time their layer shows on that car;
   - `foldAt` returned a new object per vertex of every folded part on each re-skin; the vertex
     loop is now a small standalone `skinVertices` (V8 optimises it; inside the big `update` it
     stayed in an unoptimised tier where every float op allocated);
   - `InstancedCar.meshes` was a getter building an array, called per instance per frame;
     the fleet round-tripped colours through `Color.getHex()`; the LOD director built a state
     object, two arrays, a closure and a key string per car per frame (now pooled; the LOD log
     copies what it keeps); props re-pick used objects + `Math.hypot`; `syncViews` / the livery
     map allocated per frame.
7. **HUD canvases**: below 8 fps the minimap and telemetry plots redraw every other frame
   (each redraw is a canvas upload + composite of ~10 ms under SwiftShader).

Measured and **not** changed: the post stack (bypassing it saves ~10 ms of synced render but not
of the real frame, and would change the look), the siren light on low profiles (~0 on the real
frame), a desynchronized canvas (within noise, risks tearing on GPUs), `preserveDrawingBuffer`
(no effect).

## Per-feature cost lines (for attributing the next regression)

- `src/perf.ts` frame-loop sections now: input, sim, **crash** (replay director), views,
  events, fx, traffic, weather, camera, audio, ui, **map** (minimap), **layout** (HUD layout
  pass), **fleet** (instanced traffic / police), **props** (parked cars), **shaders** (prewarm),
  render. `tagFeature(root, "city" | "lamps" | "sky" | "cars" | "fleet" | "props" | "fx" |
  "signals" | "weather")` tags scene roots.
- `e2e/frame_profile.mjs` prints, besides the old lines: draw calls and triangles **per
  feature** (every `renderBufferDirect` is charged to the nearest tagged parent; the post
  stack's passes count as `post`), raster ms per feature (synced render with that feature
  hidden, A/B — noisy, ±10 ms), panel re-skins per frame, and allocation split into **steady**
  (the frame loop) and **one-off** (car views built on promotion, first shader-program
  bindings, prewarm), with a steady-only top list.
- `e2e/perf_check.mjs` prints all of that as `INFO` lines for each run.

## Perf check (round 12b)

`node e2e/perf_check.mjs` (step 7 of `e2e/run.mjs`; `PERF=0` skips) runs chase + rain at
1280×720 and the phone profile (740×360, touch, chase) and checks: RTF ≥ 0.9, ≤ 12 frames over
250 ms, steady allocation ≤ 200 KB/frame and all of it ≤ 2400 KB, < 600 draw calls (< 300 on the
phone), ≤ 400 k triangles, ≤ 120 wasm calls, one point light, tiled city, near signal heads,
crash effects ≤ 1.0 ms (mean without the worst frame: one GC pause in that lap once read 1.8) /
≤ 5 draws, **0 material bumps**. The profiler also reports each section's worst single frame. `PERF_MIN_RTF` / `PERF_MAX_SLOW` override
the two machine-speed limits (e.g. `PERF_MIN_RTF=0.75 PERF_MAX_SLOW=26` on this container,
where the pre-12b build itself sits at 0.85–0.95 / 16–22).

## Merge with the touch-controls perf pass (`origin/lead/round12` @ 9ad97d0)

The touch-controls session fixed the same regression in parallel (`touch.md` → "Desktop perf
regression after round 12b"); the lead merged it into round 12 and this branch merged that.
Where the two overlapped, **round 12's design was kept**: its low / touch LOD distances, far
promoted cars via the fleet only on those profiles (`viewDrawM`, not on a real GPU), **every
parked car drawn with 24 full meshes** (props rule; this branch's parked-car distance cull, the
8-car cap, the lamp-pole distance and the 100 m full-mesh distance were dropped), software GL
starting at 0.35× (floor 0.25×), its siren warm render and crash-FX precompile. Kept from this
branch on top: batched panel parts, shared view geometry / materials and lazy debug overlays,
per-model panel caches + `skinVertices`, pooled LOD states, hex fleet path, 192 m prop chunks
(every car still drawn), incremental prewarm for materials that appear later (car models,
headlight pools, contact markers), sky after the opaque scene, signal / city-detail distances
on the low profiles, software rain 20 %, HUD canvases at half rate below 8 fps, the profiler,
`perf_check` and the e2e robustness fixes.

After the merge (same container, `perf_check`): chase RTF 0.758, rain 0.706, phone 0.776;
316 / 279 / 262 draw calls; 0 material bumps; steady allocation 738 / 921 / 688 KB/frame.
Lower than before the merge because **the wasm sim step doubled with the merged Rust** (round
12b / 13 vehicle merge): 120–145 ms per frame vs 65–76, self time now led by Rapier shape casts
with cylinders (`gjk::directional_distance`, `Cylinder::support_point`, `cast_shapes`,
tetrahedron point queries) — a wheel / ground cast, sim-side and out of scope here (request to
the vehicle owner). Parked cars are ~125–140 k triangles again under the 24-full-mesh rule (52 k
with 8). `e2e/menu.mjs` now fails "countersteer slider disabled until the wasm setter exists":
round 12's `main.ts` always enables the slider (`countersteer: true`), the menu e2e was not
updated — a base mismatch, not from this branch.

## Changes outside my area (round 12b)

All in `apps/testbed-web`:
- `src/render/panels.ts` (R): per-material batches of body parts, per-model part data and batch
  index caches, six shared panel materials (greybox values recorded at creation; `applyStyle`
  unchanged in effect), `skinVertices` split out of `update`, `foldAt` scratch result. No change
  to what is drawn or to `materialCentre` / debris / lens breaking.
- `src/vehicle_view.ts`: shared geometry / materials, lazy debug overlays, `lodFar`.
- `src/render/style.ts`, `src/render/post.ts` (S): sky on the far plane after the opaque scene;
  `compileOverride` + `LowPolyPost.compile` (prewarm for the HDR target); lamp poles distance;
  headlight-pool geometry per size. Colours, post and bloom unchanged.
- `src/render/fx/skids.ts` (crash FX): `forceSinglePass`. `e2e/crash_fx.mjs`: the burnout check
  waits up to 5 s more for its 8 skid segments (it slept a fixed 3.2 s of wall time and laid 6
  on a slow box in one full-suite run).
- `src/render/props.ts`, `src/render/lod.ts`, `src/render/fleet.ts`, `src/render/city_tiles.ts`,
  `src/render/quality.ts`, `src/signals_view.ts`, `src/scene.ts`, `src/ui/map.ts`, `src/main.ts`,
  `src/race_marker.ts`, `src/perf.ts`, `src/render/weather.ts` (tag only).
- `e2e/run.mjs` props check: parked cars past the draw distance count as `culledFar`; "full
  mesh near the camera" accepts the profile's `maxFull` (8 on lean profiles).
- `e2e/reset.mjs`: "heat unchanged by the reset" reads heat, resets and reads again in one
  `evaluate` (three separate calls let frames run in between; early in a chase the heat moves
  on its own: one full-suite run read 0.26 → 1.37; 3/3 alone passed before the fix too).
- `e2e/frame_profile.mjs`, `e2e/perf_check.mjs`: see above.

## Requests (round 12b)

- **Lead:** decide the machine calibration for `perf_check` (targets vs `PERF_MIN_RTF` /
  `PERF_MAX_SLOW` on slow containers; numbers above). The steady-allocation target (0.2 MB/frame)
  is not reachable at 3–4 fps with the current frame loop: what is left is a long tail
  (re-skinning dented cars in V8's Maglev tier ~100–190 KB, crash-effect wheel pass ~70 KB,
  Three internals and wasm-bindgen arrays). Per sim second it went 6–7 MB → 2.5–3 MB.
- **Lead (hash spec v4, optional):** unchanged — the per-tick replay hash is still the largest
  removable sim cost in the browser (the sim is 65–100 ms of a ~90–110 ms JS frame here).
- **S (art):** the low-poly post stack renders into a half-float target even with bloom off
  (low profile); on SwiftShader it costs ~10 ms of synced render. A matching direct path
  (tone mapping in the materials) would need the fog / sky / unlit lamps re-checked — your call.
- **R (panels):** batching keeps one mesh per lens and per wheel part; merging the wheel parts
  per wheel (they spin together) would save 4–8 more draws per car.
- **Crash FX:** `CrashFx.vehicles` still allocates ~70 KB/frame at 3–4 fps (number boxing until
  TurboFan); pooling the per-car scratch in typed arrays would remove it.

## Gates (round 12b, 2026-10-20, cloud container, `CARGO_INCREMENTAL=0`, debug info off)

| Gate | Result |
|---|---|
| `cargo fmt --all -- --check` | pass |
| `cargo clippy --workspace -- -D warnings` | pass |
| `cargo build --target wasm32-unknown-unknown -p wp_wasm` | pass |
| `cargo test --workspace --no-fail-fast` | **6 failures, all pre-existing on `lead/round12`** (this branch has no diff to c1a98ab in `crates/`, `data/`, `apps/simcli`, `apps/server`, `tools/`, Cargo files): `simcli` `goldens_match`, `wp_opponent` `live_police_chase_ends_in_escape_or_capture_without_stuck_cars`, `wp_race` `race_picker` (`hollins_hill_sprint`, `loop9_circuit`, `lantern_checkpoint_draws_the_police`) — the same set art.md / props.md / crash-fx.md report on the base. Not re-blessed here. |
| `npx tsc --noEmit` | pass |
| `just web-e2e` = `node e2e/run.mjs` (+ `e2e/race.mjs`) | **204 checks pass**, incl. menu, touch, crash replay, crash FX, weather, LOD, props, signals, art style, gamepad, reset; **fails only the perf step on the machine-speed targets** (RTF ≥ 0.9, ≤ 12 slow frames) and the steady-allocation target, see the targets table; every structural perf check passes (0 material bumps, phone < 300 draw calls). One run also hit the `reset.mjs` heat race (fixed, see above). `e2e/race.mjs`: all 19 pass. |
| Final `e2e/perf_check.mjs` | chase 0.819 (17 slow frames, 279 draws, 746 / 1012 KB), rain 0.831 (23, 295, 896 / 1263 KB), phone 0.830 (14, **264 draws**, 638 / 856 KB); 0 bumps everywhere |

# Round 12 pass (first browser perf pass)

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

## Perf e2e check (first pass; superseded by "Perf check (round 12b)" above)

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

## Allocation pass (2026-10-20)

- **Who / why:** helper for the integration lead. `e2e/perf_check.mjs` gates steady allocation at
  ≤ 200 KB/frame; it measured ~620–750 (chase), ~900 (rain), ~640 (phone).
- **Touched:** `apps/testbed-web` only (TypeScript + the profiler). **No Rust, no `data/`, no
  `wp_contracts`, no wasm API change**; nothing drawn changes and the sim is untouched.
- **Result in one line: the 200 KB/frame gate is still not met, and cannot be met by app code on
  this box.** Three.js's own render path allocates ~190–210 KB/frame here by itself (see
  "What is left"). What this pass did: it fixed the profiler so it no longer measures its own
  side effects, and it removed the app's large allocators that fire on a crash. Those were
  spikes of +50–360 KB/frame whenever a crash deoptimised them inside the measuring window. It
  also removed several real per-frame object allocations.

### Numbers (this container, headless SwiftShader, 12 s windows, steady KB/frame)

Run-to-run spread is large: ±50 KB, and up to +150 KB when a crash lands in the window. Each
cell is one run unless it says otherwise. "Before" is `bd0a3e64` (the tree this pass started
from), run from a copy of that tree with the same wasm and interleaved with the "after" runs.

| Profile | Brief's figures (old profiler, earlier, busier box) | Before, old profiler | After, old profiler | Before, new profiler (2 runs) | After, new profiler (2 runs) | **After, `perf_check` (2 runs)** |
|---|---|---|---|---|---|---|
| chase 1280×720 | 620–750 (961 in my first run, skinVertices 356) | 522 | 543 | 459 / 501 (721 in one earlier run: skinVertices 112, crash FX 57) | 483 / 491 | **475 / 460** |
| rain 1280×720 | ~900 | 614 | 620 | 527 / 616 | 479 / 499 | **507 / 492** |
| phone 740×360 | ~640 | 489 | 438 | 407 / 413 | 436 / 427 | **— (run did not finish) / 425** |

Read it like this:
- **The machine sped up.** The same "before" tree with the old profiler went from 961 (my first
  run, 2.5 fps, while the lead was compiling) to 522 (3.6 fps). Allocation per frame scales with
  sim steps per frame, and at 2.5 fps more of the frame loop is still unoptimised.
- **When nothing crashes in the window, before and after are the same within noise.** The
  pass's biggest wins are allocators that only show up when a crash deoptimises them inside the
  12 s window (skinVertices 112–356 KB, crash-FX `vehicles` 48–70 KB, `chordFolds` 10–23 KB).
  After the pass, none of them appears in any run. Before, they appear in some runs and not
  others.
- The "new profiler" columns are lower than "old profiler" mostly by the `noiseBuffers`
  reclassification (37–59 KB in the old-profiler before runs) and the probe fix.
- `perf_check` with `PERF_MIN_RTF=0.75 PERF_MAX_SLOW=26`, after: chase RTF 0.80 / 0.86, rain
  0.82 / 0.75, phone 0.90. 0 material bumps. Crash FX 0.24–0.29 ms. **It fails on steady
  allocation in all three profiles. It also fails "phone draw calls < 300" at 313.** That draw
  count is not from this pass: the before tree measures 311 / 312 (cars ~77, props ~71, city
  ~125). In the first `perf_check` run, the phone profile "did not run" (no JSON, empty
  stderr). Run alone straight after, and in the second `perf_check` run, it ran normally.

### Fixes (biggest first)

1. **Profiler artefacts** (`e2e/frame_profile.mjs`). The wasm call counters (which replace the
   `sim` / audio bridge functions) and the draw-call probe (which replaces
   `renderer.render` / `renderBufferDirect`) were installed 5 s into the chase, just before
   measuring. Replacing a constant method throws away the optimised code of every caller (V8
   "dependent field type constness changed"), e.g. Three's `renderObject`. The measuring window
   then measured that code warming up again. Both are now installed before the chase starts. The
   probe's per-feature counters are typed arrays: before, it stored a heap double per draw.
   `noiseBuffers` (the audio noise tables, built once per AudioContext on the first impact
   sound) now counts as one-off, like car-view builds.
   - New options: `WARMUP_S` (chase time before measuring, default 5), `TOP`
     (steady top-list length), `PROFILE_OUT` (save the raw sampling profile), `JS_FLAGS` (V8
     flags, e.g. `--trace-deopt`).
2. **Panel re-skin** (`render/panels.ts`). `skinVertices` used to read `Frame` matrices and
   chord-fold objects through null-or-object parameters. Before a crash its fold values are all
   integers. On the first dent it deoptimised ("Insufficient type feedback", "lost precision") into
   a tier where every float operation allocates.
   - It now reads only typed arrays: a packed `skinParams` and a packed chord buffer (the old
     `foldAt` is inlined).
   - `chordFolds` became `chordFoldsInto`: the fold JSON is packed once per part model
     (`FoldPack`), panel states per region are an `Int8Array`, and it allocates no closures,
     arrays or `Vector3`s.
   - `fitFrame` / `triFrame` write into a pooled `Frame` per region and use scratch vectors.
   - `warmSkinVertices` runs both kernels once, through every branch with fractional values,
     when the first car view is built (25 ms here). V8 then optimises them once and keeps that
     code.
   - **Checked bit-exact:** an A/B harness ran the `bd0a3e64` `PanelBody` and this one on the
     same lattice node streams (head-on, barrier and a 40 s city chase). 2653 re-skins, 79 102
     part comparisons, folds up to 0.28 m, 113 free-panel frames: 0 differences in positions,
     maxDisp, maxFold, maxPull or lens state.
3. **Crash FX per-car pass** (`render/fx/index.ts`). `CrashFx.vehicles` ran once a frame, so V8
   optimised it only ~10 s into the chase, and any new branch sent it back.
   - Its float work is now in small kernels: `carFrame`, `wheelFx`, `carPlane`, `engineSmoke`,
     `radiatorSteam`.
   - Wheel records are packed per `VehicleInfo` wheel. Wheel JSON of different car models has
     different shapes, and reading through them caused "wrong map" deopts.
   - On the first frame, `warmVehicleKernels` runs the kernels through every branch: 20 000
     iterations, ~50 ms once here. It uses the real argument objects and migrates live `CarFx`
     objects onto the generalised shape. A skid strength that only ever saw 0 / 1 kept an integer
     field and deoptimised on the first real skid.
   - The same arithmetic in the same order: emission counts, random draws and arguments are
     unchanged.
   - `e2e/crash_fx.mjs`: 0.41 KB/effects frame (was 0.45–0.6).
4. **Particle kernels** (`render/fx/pool.ts`): `ballistic` / `spin` are warmed at module load
   (11 ms).
5. **Parked-car chunks** (`render/props.ts`).
   - Every chunk's instanced meshes drew the same geometry object with a different instance
     matrix. Three keys vertex-array state by geometry + program, so each chunk draw re-bound
     its attributes and allocated a new attribute cache (`saveCache`, ~15 KB/frame).
   - Each chunk mesh now gets a `geometryView`: a new geometry object over the same attributes
     and GPU buffers, disposed with the session.
   - The near-car scan of `repick` is a typed-array loop (`nearCars`); `distTo` returned a boxed
     number per prop.
6. **Fleet colours** (`render/fleet.ts`). `putHex` (per car per frame) wrote colours through
   `Color.setHex` + `setColorAt`. It now copies memoised Float32 instance-buffer values: the same
   floats, made by the same calls. The lamp memo is cleared when the art style or night > 0.2
   changes.
7. **Audio** (`audio/index.ts`, `audio/voices.ts`).
   - The listener transform updates the camera / world matrices once and inverts once. Before,
     `getWorldPosition`, `getWorldDirection`, `getWorldQuaternion` and 3× `worldToLocal` each
     redid them; the arithmetic is the same.
   - The audio panel's sample status and engine line are only built while the panel is open. It
     returned early otherwise, but they were built every frame.
   - `collect` reaps voices with `Map.forEach` and compacts `dying` in place. Before, it used a
     `[key, voice]` array per voice and a filtered copy of the list.
8. **CG / velocity debug layer** (`vehicle_view.ts`): two `Vector3` per car per frame are now
   scratch.

### What is left (after, new profiler, chase; KB/frame)

- **Three.js render path ~190–210.** Breakdown: `renderObject` 30–40, `copyArray` 20–29,
  `setValueV3f` 11–15, `painterSortStable` 8–10, `dot` 6–11, `renderBufferDirect` 7–9,
  `projectObject` 5, plus a long tail. At 3–4 fps these functions reach TurboFan only ~25–35 s
  after page load: the trace shows `renderObject` / `setProgram` optimised ~30 s in. With
  `WARMUP_S=25` the steady total is ~376 KB, and the Three entries move to the uniform setters
  (`setValueM4` / `M3` / `V3f`).
  - This cannot be fixed from the app without patching Three or rendering faster.
  - **This part alone is at or over the 200 KB budget.**
- **Builtins ~70–90** (`next`, `max`, `min`, `sort`, `slice`): iterators and Math calls from
  frame code still in a baseline tier.
- **App long tail ~190–215.** No single item is over 15 KB:
  - `audioBridge.frame` JSON parse 11–14. Needs a binary `audio_frame` from wp_wasm and a new
    consumer for its commands. Not done: it is a protocol change across the audio code.
  - Telemetry plot 11–14 and HUD silhouette 5–8: canvas `lineTo` / `fillRect` with float
    arguments are boxed per call. Snapping to integers would change pixels.
  - Audio voices / siren scheduling 15–20, LOD director 7, `newContactMarker` 4–8,
    `asymShaper` 3–6 (a cache fill per new curve), and the frame loop itself.
- wasm-bindgen arrays are small here: `getArrayF32FromWasm0` ~3 KB/frame.

### Requests

- **Lead:** decide what the steady-allocation gate should mean on a 3–4 fps software-GL box.
  - With a 5 s warm-up it mostly measures V8 tier-up of Three's render path; even at 25 s it
    is ~376 KB. Either measure after a longer warm-up (`WARMUP_S`), or gate on the app's own
    share (the `steadyTop` entries outside the Three chunk), or keep 200 KB for real-GPU runs
    only.
  - The phone draw-call limit (< 300) fails at ~311 on the base tree too.
- **wp_wasm / audio (optional):** a buffer-returning `audio_frame` (commands as a packed
  `Float32Array` + string table) would remove the per-frame JSON parse (~12 KB/frame here).

### Gates (this pass)

| Gate | Result |
|---|---|
| `npx tsc --noEmit` | pass |
| `PERF=0 node e2e/run.mjs` (includes weather, gamepad, reset, crash FX, crash replay, touch, menu) | **181 PASS, 0 FAIL** |
| `node e2e/crash_fx.mjs` | 14 pass |
| `node e2e/audio.mjs` / `audio_impacts.mjs` / `audio_engine.mjs` | 12 / 49 / 125 pass |
| panel re-skin A/B vs `bd0a3e64` (scratch harness, not committed) | 0 differences over 79 102 part comparisons |
| `PERF_MIN_RTF=0.75 PERF_MAX_SLOW=26 node e2e/perf_check.mjs` | **fails**: steady allocation, all 3 profiles (475–507 / 460–492 / 425 KB); phone draw calls 313 (base 311–312). Every other check passes. |

The audio e2e runs re-render `docs/media/audio/*` (wav / jpg / README numbers move a little from
run to run). Those files were restored from HEAD; this pass does not change them.

## Car detail (2026-10-26, owner: "make traffic and other racer LOD render high quality further out")

`render/quality.ts` `CAR_DETAIL`, game menu Settings → Graphics → "Car detail" (`?cardetail=near|far|max`):

| Level | Full per-panel view (promoted traffic, police, roaming street racers) | Mid-LOD instanced mesh |
|---|---|---|
| near (old high default) | 30 m | 250 m |
| far (new high default) | 80 m | 400 m |
| max | 160 m | 800 m |

"From quality" (default) = far on the high profile (and auto on a hardware GPU); the low,
software-GL and touch profiles keep 30 m / 120 m (touch: never a view), so `perf_check.mjs`
(auto under SwiftShader = low) is unchanged. Picker-race AI racers and the player always use
their full view. Traffic is only a sim vehicle within 80 m of a player (wp_world
`physical_radius_m`), so a view distance past 80 m only adds police and street racers.

Measured on the combined world (`ashpeak`, 1280×720, headless SwiftShader, gfx=high; A/B in one
page, so frame times are raster-bound noise here and draw calls / triangles are what count):
- A car in its full view costs ~50 draw calls and ~13k triangles (free roam with 16 roamers, all
  in view: 1727 calls / 877k tris vs 907 / 667k in free drive), plus its lattice re-skin.
- Night-run chase: 30→80 m view distance changed nothing (no traffic within 80 m at 130 km/h);
  mid mesh 250→400 m: 395k→507k triangles (fleet 31k→145k), draw calls unchanged (instanced);
  250→500 m: 531k.
- Free drive at the Port Vesper spawn: 30→80 m added 0–1 views (≈ +20–50 draw calls).
Worst case on "far": every promoted traffic car (≤ 40 sim slots) plus police near the camera in
view, ~50 calls each; hence a setting rather than a fixed raise.
