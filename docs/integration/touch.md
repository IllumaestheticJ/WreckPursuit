# Touch controls (`lead/touch-controls`)

- **Who:** helper for the integration lead, 2026-10-20 (owner request, row 71: phone and tablet
  touch controls, so people sent the public demo can play on a phone).
- **Branch:** `lead/touch-controls` from `origin/lead/round12`.
- **Touched:** `apps/testbed-web` only (TypeScript + e2e) and this doc. **No Rust, no `data/`,
  no `wp_contracts`, no sim changes.**

## What the player gets

On a touch device (coarse primary pointer, or the first touch on the page) the testbed and the
demo page show on-screen controls. Landscape layout:

| Where | Control | How |
|---|---|---|
| bottom-left | steer ◀ ▶ | two big buttons; the finger's position across them is the analogue value (outer part = full lock, sliding inwards steers less, across the middle switches side); slewed at 8 /s so a tap is not a 0→1 jump |
| bottom-left (tilt mode) | tilt to steer | phone held like a wheel: gravity's angle in the screen plane, 3° dead band, full lock at 32° |
| above the steering | 🎥 camera · ↺ reset · STEER ◀▶ / STEER TILT | edge buttons; the last one switches steering mode |
| bottom-right | GAS (tall), BRAKE | analogue by the finger's height on the pedal: bottom 0.3 → top 1.0; slide to modulate |
| left of the pedals | HAND BRAKE | held |
| top-right | 🗺 map · ⏸ pause | edge buttons |
| portrait | "Rotate your phone" | full-screen hint (tap to play anyway) |

Multi-touch works (pedal + steering + handbrake at once; each finger is captured by the control
it went down on). Hit areas: steering buttons and pedal widths `clamp(64px, 24vh, 104px)`,
pedal heights 1.35× / 1.9× that, small buttons 44 px. Everything is offset by
`env(safe-area-inset-*)`. A touch on the open 3D view still drags the camera (look-around).
While paused, in a replay, a bench or with the full map open, only the map / pause buttons
remain (everything held is released). Blur / tab hidden / `pointercancel` release everything.

**Steering mode** is remembered in `localStorage` `wp.touch.v1` (`{ "steer": "buttons" | "tilt" }`).
Tilt asks for motion permission where the browser needs it (iOS: `DeviceOrientationEvent.requestPermission`,
must be called from a tap) and **falls back to buttons** when it is denied, unsupported, or when
no orientation events arrive within 1.5 s (e.g. a cross-origin iframe without the
`accelerometer`/`gyroscope` permission policy — the artifact embed may be one).

URL: `?touch=1` forces the controls on (desktop testing; real touches still needed to drive,
mouse clicks on the controls work too), `?touch=0` never installs them.

## How it plugs in

- `src/touch.ts` (new): `TouchControls`, `installTouch()`, `prefersTouch()`,
  **`setTouchSteerMode(mode): Promise<mode in effect>`** and `touchSteerMode()` (for machine M's
  pause menu / settings; also on `window.__wp`). Pure helpers `tiltSteerAngle`, `tiltToSteer`,
  `buttonSteer`, `pedalValue`. Injects its own CSS (so `tools/demo/index.demo.html` needs no
  change).
- `src/input.ts` (**minimal hook**, machine C please keep when merging clutch/reset):
  `InputSource` gains `"Touch"`; `PadRead` is exported as `AnalogRead`; new
  `setTouchReader(fn)`; `readInput` calls it once per frame like `readGamepad`. Priority: pad
  active > touch active > keyboard; touch edges (camera, reset, map, pause) are merged with the
  pad's into `edges`, so main's `doAction` runs them. Touch values go to the sim **as read** —
  no keyboard ramps — so the analogue assist filters apply exactly as for the gamepad.
- `src/main.ts`: source `"Touch"` → `assistPreset("Gamepad")` + touch defaults **auto gears,
  auto clutch, TCS, steer help** (`steer_speed_sensitivity` 1.0, `countersteer_help` 0.6), then
  the viewer's saved assist-box choices (`wp.assistOverrides`) on top. (The owner's desktop
  default "TCS / auto gears off" does not apply to touch.) `touch.setDriving()` each frame.
- Sim side unchanged: the touch input is an ordinary `DriverInput`, logged in replays like any
  other.

## Phone performance

`src/render/quality.ts`: on a touch device `auto` = new **touch profile** (`?gfx=high|low` still
override): the `low` profile plus pixel ratio capped at **1.0** (governor down to 0.3× native),
600 m city draw distance, 20 % rain streaks, **traffic drawn within 300 m**
(`LodDirector.trafficDrawM`), full car meshes only within **120 m** (`Fleet.fullLodM`), and
parked cars: full mesh for at most **24** within 120 m, **proxies only within 350 m**
(`PropView.drawM`, `maxFull`, `fullLodM`). Desktop defaults are unchanged (all `Infinity` /
250 m / 256).

The parked-car change mattered most: round 12b draws all **3836** parked cars of Port Vesper as
one un-culled InstancedMesh (+ up to 256 full meshes within 250 m): ~1.2 M of the frame's
~1.36 M triangles. With a finite `drawM` the proxy instances are compacted to the near cars
(re-picked every 12 m of camera travel; a zero-scaled instance still costs vertex work on a
weak GPU). Every prop within 150 m keeps exactly one drawn instance (e2e checks it), so no
invisible collisions.

Measured with `e2e/frame_profile.mjs`, now with `TOUCH=1` (hasTouch + isMobile + DPR 3 → touch
profile) and `CPU_THROTTLE=n` (CDP CPU throttling), city chase, 740×360, 12 s, headless
SwiftShader on a 4-vCPU cloud box (CPU rasterizer: numbers are only relative to each other):

| City chase @ 740×360 | desktop `auto` profile | touch profile |
|---|---|---|
| no throttle: real-time factor / fps | 0.52 / 2.1 | **0.96 / 5.3** |
| no throttle: triangles / GPU-synced render | 1.36 M / 432 ms | 326 k / 154 ms |
| CPU ×2 ("mid-range phone"): RTF / fps / frame p50 | 0.46 / 1.8 / 550 ms | **0.97 / 5.1 / 183 ms** |
| CPU ×4: RTF / fps | 0.47 / 1.9 | 0.58 / 2.4 |
| CPU ×2: JS per frame (sim / views / render submit) | 113 / 13 / 35 ms | 104 / 7.5 / 16 ms |

Headless fps is dominated by SwiftShader rasterizing on the CPU; on a phone GPU the raster cost
goes away. Estimate at CPU ×2 (a phone core about half as fast as this box's): sim ≈ 2.3 ms per
240 Hz tick incl. police + traffic + Rapier ≈ 19 ms per 30 fps frame, plus per-frame JS (views
7.5, render submit 16 — partly SwiftShader driver work —, audio 3, UI 2.6) ≈ 30 ms → **~20–30
fps**; the 30 fps target holds on phones close to desktop single-core speed, and real time (no
slow motion) holds at CPU ×2 either way (RTF 0.97 above). At CPU ×4 the wasm sim step alone
exceeds real time (needs sim-side work, not in scope here — see Requests).

Reproduce: `cd apps/testbed-web && VIEWPORT=740x360 TOUCH=1 CPU_THROTTLE=2 node e2e/frame_profile.mjs chase 12`.

## Small-screen layout

CSS injected by `touch.ts`:
- `@media (max-width: 960px), (max-height: 500px)`: dev panels hidden (`#controls`, telemetry,
  components, regions, event feed, input panel, damage silhouette, audio panel, weather line)
  — on any small screen, touch or not.
- With touch on (`html.wp-touch`): HUD box shows only speed + gear (top-left), minimap
  (`clamp(84px, 24vh, 128px)`) under it on the left, chase readout top-centre at 12 px / 180 px
  wide, pursuit banner at 30 % with `clamp(24px, 8vh, 44px)` text, race HUD top-right under the
  map / pause buttons. The demo page's "Menu" button stays bottom-centre (between steering and
  handbrake at every tested width).
- e2e checks speed, minimap, chase readout, pursuit banner (forced visible) and every touch
  control are on-screen and pairwise non-overlapping at **640×360, 740×360, 932×430** (the
  landscape forms of 360×640 … 430×932). Screenshots: `TOUCH_SHOTS=dir npm run e2e:touch`.

## e2e

`e2e/touch.mjs` (`npm run e2e:touch`; step 9 of `e2e/run.mjs` = `just web-e2e`): emulated
phone (Playwright `hasTouch`, `isMobile`, DPR 3, 740×360) with **real multi-touch through CDP
`Input.dispatchTouchEvent`**. 22 checks: controls shown + touch quality profile + dev panels
hidden; a swipe up the throttle is analogue (0.33 → 0.97) and accelerates (~18 m/s in 2.5 s);
source `Touch` with auto gears + TCS + steer help; steering with a second finger while on the
throttle (sim applied steer 1.0), analogue drag (−0.44); release lets go; brake + handbrake;
camera / map (driving controls hidden while open) / pause buttons; props within 150 m all drawn
while far parked cars are not instanced; tilt falls back to buttons with no orientation events
and steers ±0.76 with synthetic ±25° events; no overlaps at three sizes; portrait rotate hint;
a 1280×720 desktop shows no touch UI (keyboard source, no touch profile); no page errors.

## For machine C (`input.ts` / `bindings.ts`: clutch, reset)

The touch hook is ~25 lines in `input.ts` (see "How it plugs in"). Touch sends `clutch: 0` and
no gear edges (auto gears is a touch default); if clutch becomes analogue, touch keeps 0. The
reset button sends the `reset` edge action, so whatever `doAction("reset")` becomes applies.

## For machine M (`ws/ui`, pause menu / settings)

- Steering choice: `import { setTouchSteerMode, touchSteerMode, touchControls } from "../touch"`;
  call `setTouchSteerMode("tilt")` **from the tap handler** (iOS permission), use the resolved
  value (it may come back `"buttons"`), `touchControls()?.onModeChange(cb)` to follow the
  automatic fallback. Show the option only when `touchControls()?.visible`.
- The touch ⏸ button sends the `pause` action (same as P / pad Menu). While `app.paused` the
  driving controls hide and only map / pause stay, at z-index 20 under your menus if they use
  a higher one (the race picker uses 80, demo menu 50).

## Changes outside my area
None outside `apps/testbed-web` + this doc. Inside testbed files owned by others:
`src/render/props.ts` (`fullLodM`, `maxFull`, `drawM` + compacted proxies; defaults unchanged),
`src/render/fleet.ts` (`Fleet.fullLodM`), `src/render/lod.ts` (`trafficDrawM`),
`src/render/quality.ts` (touch profile), `e2e/frame_profile.mjs` (`TOUCH`, `CPU_THROTTLE`).

## Requests
- **Lead (perf, desktop too):** the round-12b parked cars cost the **desktop** chase half its
  frame rate under headless SwiftShader (1280×720: RTF 0.50, 1.46 M triangles vs. 0.95 / 340 k
  in browser-perf.md), so `e2e/perf_check.mjs` (RTF ≥ 0.75, ≤ 700 k triangles) fails on the
  round-12 base. Setting `props.drawM` (e.g. 600 m, the low profile's city draw distance) in the
  software-GL `auto` / `low` profiles would recover it; left unchanged here as it is outside the
  touch scope.
- **Lead (sim cost, phones):** at CPU ×4 the wasm sim step alone (~1.3 ms/tick unthrottled with
  police + traffic) cannot keep real time; the per-tick replay hash request in
  `browser-perf.md` would help most.

## Gates (2026-10-20, cloud container, base `origin/lead/round12` @ `091cd34`)
With `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0`:
- `cargo fmt --all -- --check` ✔ · `cargo clippy --workspace -- -D warnings` ✔ ·
  `cargo build --target wasm32-unknown-unknown -p wp_wasm` ✔ · `tsc --noEmit` ✔
- `cargo test --workspace --no-fail-fast`: 952 pass, **4 fail on the base** (no Rust changed on
  this branch): simcli `goldens_match`, wp_race `grid_highway_sprint` ("only 2 of 4
  finished"), wp_sim `vesper_v2_cars_drive_up_the_kerb_and_along_the_sidewalk`, wp_wasm
  `race_tests::short_sprint_with_ai_runs_to_results_and_pays_through_the_economy`.
- `just web-e2e`: **109 PASS**, incl. all 22 touch checks, gamepad, weather, LOD and props.
  **1 FAIL: `e2e/perf_check.mjs` on the desktop profile** (chase / rain RTF 0.48, 1.40–1.45 M
  triangles, 22 frames > 250 ms). Same on the untouched base (`origin/lead/round12` in a
  worktree: RTF 0.50, 1.46 M triangles): the round-12b parked cars, see Requests. Desktop
  render settings are unchanged on this branch.

## Follow-up after the round-12b merge (2026-10-20, with M's game menu / HUD layout)
Lead report: on the merged tree `e2e/touch.mjs` failed 6 of 22. Causes and fixes:
- **Assists (auto gears / TCS off on touch):** the game menu seeds its settings on first load
  from the desktop defaults (TCS / auto gears off) and stores them in `wp.assistOverrides`,
  which then won over the touch defaults. `seed()` now uses the touch defaults on a touch device
  (auto gears + TCS on); what the player changes in the menu still wins.
- **Pause / tilt / layout:** the ⏸ button already routes to `menu.openPause()` (via the `pause`
  action), but the touch layer (z-index 20 at page level) drew *over* the menu (z-index 90
  inside `#overlay`'s stacking context), and the test's second ⏸ tap was swallowed by
  `setInputBlocked` — the menu stayed open, so tilt read 0 and the layout checks found the
  driving controls hidden. Now `TouchControls.setCovered()` hides the whole touch layer while
  the game menu, controls panel, garage, run-end panel or race picker is up; the e2e taps ⏸
  (menu opens, controls hidden) then the menu's **Resume** button by touch (game resumes,
  controls back). M's `layout.ts` needed no change (the touch CSS keeps the minimap top-left).

Gates on the merged tree (+ this fix): `e2e/touch.mjs` 22/22, `e2e/menu.mjs` all pass, `tsc` ✔.
`e2e/perf_check.mjs` (desktop) fails **on the merged base too** (A/B with this fix stashed:
chase RTF 0.63, rain 0.59, 23 frames > 250 ms, 2.5–2.8 MB allocated / frame); one run also
showed an intermittent "6 material version bumps (MeshBasicMaterial)" in steady frames.
Phone profile on the merged tree (740×360, touch): RTF **0.71** unthrottled / 0.65 at CPU ×2
(was 0.96 / 0.97 before the merge) with similar triangles (~285 k): render submit 36 ms
(was 12), SwiftShader raster 164 ms (was ~100), 659 draw calls — the new art / effects pass,
shared with desktop. **Request (lead / S):** look at the round-12b render-submit and raster
regression; the touch profile's own settings are unchanged.

## Desktop perf regression after round 12b (2026-10-20, owner: "fix the desktop perf regression too")
`e2e/perf_check.mjs` (1280×720, headless SwiftShader = the software-GL `auto` profile) failed 6–7
checks on the merged `lead/round12`. Same-container reference: `lead/browser-perf` (the perf pass,
which passed where it was written) built and profiled here gets chase RTF 0.87 / 18 frames >
250 ms, rain 0.95 / 12 — **this container is slower than the one the thresholds were set on**,
so "frames over 250 ms" is partly machine-dependent.

Found (probes: GPU-synced render per scene group, per-frame timeline, CPU profile, program log):
1. **Skid marks re-resolved their shader every frame** (transparent + DoubleSide without
   `forceSinglePass`: Three flips `side` twice per draw; with its `onBeforeCompile` that is
   ~0.4 MB/frame of string `replace` and the intermittent "material version bumps" failure).
   Fixed there and in the same pattern on the race-gate beam and the debug body box.
2. **The siren light recompiled every lit material mid-chase** (Three bakes the point-light
   count into lit shaders; the first police lights built ~13 programs at once, seconds under
   SwiftShader). `SceneView` now does one hidden render with the siren in the other state a few
   frames after each session start / city load (`warmLightVariants`). A first attempt with
   `renderer.compile` also compiled every *hidden* debug helper (it walks invisible objects):
   dropped.
3. **Crash effects compiled their shaders on the first crash** (meshes hidden until used).
   `SceneView.precompile(fx.group)` draws only that subtree (others hidden; empty instanced
   meshes build programs without drawing) for both siren states, a few frames after load.
4. **Geometry / draw-call bound raster** (the render barely scales with pixels): the low
   profile (software GL `auto`, `?gfx=low`) now has its own LOD distances — full car meshes
   within 150 m (was 250), traffic drawn within 600 m, parked cars: proxies within 150 m (was a
   fixed 300; two-box cars beyond — every parked car stays drawn, the props rule against
   invisible collisions; a 600 m cutoff and an 8-full-mesh cap were tried and reverted because
   `e2e/run.mjs` enforces that rule), street lamps within 450 m,
   and promoted traffic / police cars past 80 m drawn by the fleet (one instanced draw) instead of
   their own view (~40 draw calls each; `LodDirector.viewDrawM`, 10 m hysteresis). The touch
   profile gets tighter values (60 m views, 120 m proxies, 300 m lamps, 300 m traffic). High profile (a real GPU)
   unchanged. The LOD distances now also follow a live graphics change from the game menu
   (`Quality.onChange`), and `setMode` keeps the touch pixel-ratio cap (it had been lost in the
   merge).
5. **Governor resizes**: software GL started at 0.5× and stepped down 15 % at a time, each step
   resizing the canvas and post targets (~0.7 s of `setSize` in an 8 s window). It now starts at
   0.35× (where it always ended) with a floor of 0.25×.

Results (same container, perf_check / frame_profile, 12 s windows):

| | chase RTF · >250 ms · KB/frame | rain RTF · >250 ms · KB/frame |
|---|---|---|
| merged `lead/round12` before | 0.63–0.70 · 23 · 2480–2500 | 0.59–0.69 · 23 · 2790–2850 |
| after this pass | **0.89 · 18 · 1700** | **0.80–0.86 · 21–24 · 2030–2260** |
| reference `lead/browser-perf` | 0.87 · 18 · 1990 | 0.95 · 12 · 980 |

Full `just web-e2e` after the pass (same container): **198 PASS**; perf_check chase RTF 0.865,
rain 0.773, 570 / 517 draw calls, 336k / 310k triangles, 0 material bumps; fails: frames over
250 ms (18 / 23) and once rain allocation 2439 KB (≤ 2400; 2030–2260 in other runs); and
`e2e/crash_fx.mjs` "burnout lays skid marks" (6 segments, needs 8 in 3.2 s — fails the same on
the untouched merged base: frame-rate dependent). perf_check now fails only "frames over 250 ms" (both chases; ≤ 12 allowed), which the reference
build fails here too for the chase. What is left is not render-side: late in the chase 15–22
traffic / police cars are promoted to full sim vehicles and the wasm sim step grows from ~0.65 to
~1.4 ms per tick (sim ~55–80 ms of a ~230 ms frame), plus one-off mesh uploads when cars are
promoted. Requests: **lead** — re-measure the perf_check thresholds on the CI box (or count
frames over 250 ms relative to the reference); **world / police** — the promoted-vehicle count
in the chase (sim cost per tick); **R (panels)** — `PanelBody.update` still allocates
~0.24–0.36 MB/frame (fold / frame fit) on crashed cars.

Rust gate on this branch (no Rust / data changed vs `lead/round12` @ `608a50a`): fmt ✔, clippy
`-D warnings` ✔, wasm build ✔; `cargo test --workspace`: 1001 pass, 5 fail on the base —
simcli `goldens_match`, `live_police_chase_ends_in_escape_or_capture_without_stuck_cars`, race
`hollins_hill_sprint`, `loop9_circuit`, `lantern_checkpoint_draws_the_police`.
