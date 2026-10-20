# Integration: art (machine S, prompt 11)

Branch `ws/art`. Owns `docs/art/`, `tools/assets/`, `apps/testbed-web/src/render/` materials,
palette and post.

- **Workstream / prompt:** 11 — art direction (machine S)
- **Branch:** `ws/art` (based on `origin/lead/round12` this round)
- **Folders owned:** `docs/art/`, `docs/media/art/`, `tools/assets/`, `apps/testbed-web/src/render/{palette,style,post}.ts`
- **Contract version built against:** 0.3.0 (no contract or Rust changes)

## Merge of `lead/round12` (round 12b, 2026-10-20)

`origin/lead/round12` merged into `ws/art` (commit 2421dc0 + follow-ups). The style was
ported onto the new structure, not restored:

- **Fleet / LOD** (`render/fleet.ts`, `render/lod.ts`): the instanced fleet takes the palette.
  `styledPaint(hex)` maps the greybox traffic liveries one to one onto
  `PALETTE.trafficPaints` (same variant → same slot) and police white onto `policePaint`;
  `styledLamp` turns TAIL / BAR / HEAD into HDR instance colours (brake ×`brakeGlow`,
  running lights at night, hazard amber, light bars ×`policeRedGlow` / `policeBlueGlow`,
  headlights ×`headlightGlow` at night). Both run inside `InstancedCar.put`, so every user of
  `InstancedCar` gets them. Paint / detail sheen match R's panels in low-poly dark (roughness
  0.85, metal 0.15) and detail colours (glass, trim, tyres, rims) come from the palette, so a
  promotion swaps the drawer with no recolour: `VehicleView.applyStyle` uses the same
  `styledPaint` for traffic / police (players keep `vehiclePaint`). Models are rebuilt on a
  style switch (meshes are cached). LOD e2e: 0 missing frames, max hand-over jump 0.003 m.
- **Siren light:** the one shared `SceneView.setSiren` light takes `PALETTE.policeRed/Blue`
  and `sirenLightIntensity(glow)` = glow × `PALETTE.sirenLightPerGlow` (2) in low-poly dark
  (25 as before in greybox); light-bar emissives = `policeRedGlow` / `policeBlueGlow`.
  Helicopters keep `policeGlow`. Still exactly 1 point light.
- **scene.ts:** CityTiles culling kept; `render()` updates the tiles, then calls
  `renderOverride` (post stack) or plain render. The old agent-box hooks (`agentMaterial`,
  `agentColour`) are gone with `updateAgents`. Constructor `{ postAA }`: canvas MSAA =
  `quality.antialias && !postAA`; the post target's MSAA = `quality.antialias` (and not
  `?msaa=0`), so the low profile has no MSAA anywhere and high has exactly one MSAA buffer.
- **vehicle_view.ts:** proxy stand-in (recoloured with the livery) and headlight pool kept.
  **signals_view.ts:** lamp boost kept on top of near-head packing. **panels.ts:**
  `applyStyle` kept next to the re-skin skip; glass keeps `forceSinglePass`.
- **Street lamps** read every kerb tile (the city is tiled before `onCityLoaded`), and are
  now built per 320 m city-tile cell (pole + arm merged: 3 instanced meshes per cell),
  frustum-culled and hidden past the draw distance like the city tiles
  (`style.lampTileStats(city)`). One city-wide instanced set of 1837 lamps, never culled,
  cost more than the whole greybox frame under SwiftShader.
- **Low graphics profile** (`quality.lite`, new field = the low profile; auto on a software
  GL): no light pools, bloom off (debug toggle can re-enable); tonemap / grade stay. High
  keeps everything. `?style=greybox|lowpoly`, the selector and `__wp.setStyle` unchanged.
- **Drawn props (`lead/draw-props`): not in `lead/round12` yet**, so nothing to port in this
  merge. They are ready for it: props reuse `InstancedCar` and the fleet's materials, so
  parked cars get palette liveries and lamp colours through `put` with no props change
  (`fleet.ts` here already has draw-props' `export class InstancedCar`, `put()`, exported
  `FleetMaterials` and public `mats`; only the `put` body differs → a small conflict for the
  lead: keep this side's body). Cones: `PALETTE.propCone` / `propConeGlow` exist; see Requests.

### Checks (2026-10-20, cloud container, after the merge)
- `cargo fmt --check`, `cargo clippy --workspace -- -D warnings`, `cargo build --target
  wasm32-unknown-unknown -p wp_wasm`, `npx tsc --noEmit`: pass.
- `just web-e2e` (`npm run e2e` after `npm run wasm`): **all 89 functional checks pass**
  (art style, new fleet check, LOD, signals, weather, gamepad, races, garage…); **only the
  perf step fails**, and it fails the same way in greybox on this machine (below).
- `cargo test --workspace --no-fail-fast`: 7 tests fail in `city-import`, `simcli
  scenarios` (goldens), `wp_race race_picker`, `wp_sim sidewalk`, `wp_vehicle
  handling_balance`, `wp_wasm` (2 race tests). This branch has **no** diff from
  `lead/round12` in `crates/`, `data/`, `apps/simcli`, `apps/server`, `tools/` or Cargo files,
  so all of them are the base branch's.

### Perf against the browser perf pass's budget
`e2e/perf_check.mjs` now measures `?style=lowpoly` (env `QUERY` overrides) and tags runs with
the style; `e2e/frame_profile.mjs` counts frames by the main scene render (the post stack
renders several times per frame, which divided the draw-call / triangle averages) and its
synced render / churn check go through `SceneView.render` (post included).

This container is ~3× slower than the perf pass's: the wasm sim step alone is 70–106 ms per
frame here (28.8 ms in browser-perf.md), so **greybox fails `perf_check` here too**
(chase RTF 0.73–0.76, rain 0.82, 21–29 frames over 250 ms vs ≤ 12). Low-poly dark vs greybox
on the same box (12 s chase, auto = software-GL low profile, 0.35×):

| | greybox | low-poly (merge, before fixes) | low-poly (lamp tiles + lite) |
|---|---|---|---|
| chase RTF (3 runs / 2 runs) | 0.73–0.76 | 0.66–0.68 | 0.68–0.75 |
| rain RTF | 0.82 | — | 0.73–0.77 |
| SwiftShader raster wait / frame | 151–162 ms | 245–256 ms | — |
| draw calls / triangles (chase) | ~648 / 289k | ~537 / 333k | ~656 / 286k |
| point lights / programChurn / tiles drawn | 1 / 0 / ~90 of 687 | same | same |

Static frame (`node e2e/art_cost.mjs`, one paused night-chase frame, median of 8 synced
renders, low profile): low-poly dark 160–170 ms vs greybox 119 ms (it was 254 vs 112 before
the lamp tiles). Remaining parts, each ≈ 10–40 ms and within run noise: post composite,
lamp tiles, buildings, fleet. With `gfx=high` (native res, MSAA) SwiftShader multisamples
the half-float post target in software: 815–875 ms vs 393 ms greybox (whose canvas had no
MSAA in that session); on a hardware GPU the post estimate stays ≤ 1 ms (numbers above).
**On a box where greybox meets the perf budget, expect low-poly to cost ~0.05 RTF in the
chase and ~0 in rain** — I could not show a pass here because the base fails.

### Screenshots
`node e2e/art_shots.mjs` now loads `gfx=high` (full look; `GFX=` overrides) and pins the
light-bar strobe phase for the police shots (at ~1 fps all three could land in the dark
phase). Re-shot after the merge: `docs/media/art/{dawn,day,dusk,night}_chase.png`,
`night_chase_greybox.png`, `night_police_{0,1,2}.png` (red, blue, gap), downtown street /
aerial, `rainy_night_chase.png`.

## Status (round 12 assignment, 2026-10-19) — all six items done
1. **Shared palette** `apps/testbed-web/src/render/palette.ts`: streets, district looks
   (3 building tiers, ground, window colour + lit share, sodium/LED lamps), window warm/cool,
   lamp sodium/LED, signals, car paints, traffic paints, police red/blue, and time-of-day
   keyframes (sky, horizon/fog colour + density, lights, bloom threshold, exposure) for dawn /
   day / dusk / night. Base values from the picker's "low-poly dark" mode.
2. **The look on the testbed** (`render/style.ts`, default; `render/post.ts`):
   flat shading + palette on every greybox material, building shader (per-district façade
   tiers from `map_static_json` zones, procedural windows glowing at night, fading to their
   average with distance), 1837 instanced street lamps along the kerbs in Port Vesper (4 draw
   calls) with light pools, gradient sky dome, palette fog per hour, HDR target + bloom on
   lights only (threshold above any lit surface) + ACES. R's panel meshes keep their damage
   (paint/glass/trim recoloured, still flat-shaded). Weather still works on top: wet-road
   patch untouched (only colours change), weather fog wins when thicker, clouds grey the sky
   (`docs/media/art/rainy_night_chase.png`).
3. **Lights:** police bars glow 13/15 (≥ 3× any other light; checked in `e2e/run.mjs`), brake
   lights 3, headlights 2.4 + a ground pool ahead of the car at night, signals 2.5.
4. **Perf + toggle:** "style" selector in the testbed panel (greybox / low-poly dark), also
   `?style=greybox|lowpoly`, remembered per browser; `?msaa=0` drops the post target's MSAA.
   Numbers below.
5. **Screenshots** `docs/media/art/` (dawn/day/dusk/night chase, night greybox, police bar
   phases, downtown street + aerial, rainy night) — `node e2e/art_shots.mjs`.
   `docs/art/ART_BIBLE.md` (short).
6. No `wp_contracts` / Rust edits.

## Checks (2026-10-19, cloud container, ws/art = lead/round12 + this work)
- `cargo fmt --check`, `cargo clippy --workspace -D warnings`, wasm build of `wp_wasm`: pass.
- `npx tsc --noEmit`: pass. `just web-e2e`: all PASS, including the new art-style check and
  `e2e/weather.mjs`.
- `cargo test --workspace --no-fail-fast`: 2 targets fail, **both already failing on
  `origin/lead/round12`** (this branch has no Rust, data or scenario changes):
  `simcli --test scenarios` (`goldens_match`, `all_scenarios_pass…`: hash mismatches in
  `city/blocked_parked_player_{grid,vesper}`, `golden/city_chase`,
  `golden/night_pursuit_escape`, `panels/frontal_56_hood`; vesper `traffic_turnarounds` 0)
  and `wp_net --test vesper_e2e` (`four_clients_three_minutes_in_port_vesper`). These look
  like the round-12 cross-merge failures in the lead's feedback (D, E, F).

## Perf numbers
`node apps/testbed-web/e2e/art_perf.mjs` (night city chase, Port Vesper, headless Chromium on
SwiftShader = a software GPU; relative numbers only).

| Measurement (SwiftShader, 1280×720) | greybox | low-poly dark | low-poly dark `?msaa=0` |
|---|---|---|---|
| frame (driving, 8 s) | 769 ms | 881 ms (+15 %) | 591 ms (−23 %) |
| post stack alone (same frame, GPU-synced) | — | +45 ms over 558 ms scene (8 %) | same |
| post target 4× MSAA (SwiftShader multisamples in software) | canvas MSAA | +489 ms | — |

Bloom on/off makes no measurable difference; the post passes are ~8 % of a scene render and
the rest of the delta is MSAA, which SwiftShader does in software. **Laptop GPU estimate** at
1920×1080: bright pass 0.5 Mpx × 4 taps, downsamples + 4 blurs ≈ 0.4 Mpx × 5 taps, composite
2 Mpx × 3 taps ≈ 10 M fetches + 2.6 Mpx written: ~0.2–0.4 ms on an integrated GPU, **≤ 1 ms**.
The MSAA target replaces the canvas MSAA (the canvas is created without antialiasing when the
page starts in low-poly dark), so AA costs about what it did before, at twice the bytes
(half-float). If that ever shows on a phone, `?msaa=0` is the switch.

## Changes outside my folders (minimal wiring)
- Round 12b merge: `render/fleet.ts` (styledPaint / styledLamp, exports as draw-props),
  `render/quality.ts` (`lite`), `e2e/frame_profile.mjs` + `e2e/perf_check.mjs` (measurement
  fixes above), `e2e/run.mjs` (fleet art check). New `e2e/art_cost.mjs`.
- `src/scene.ts`: `hemi`/`sun` public; `agentMaterial`, `agentColour`, `policeGlow`,
  `renderOverride` hooks; `onCityLoaded(root, cityId)`; constructor `{ antialias }` option.
- `src/vehicle_view.ts`: style re-apply on `artStyle.version`, lamp/light-bar glow, headlight pool.
- `src/signals_view.ts`: `setLampBoost(k)`.
- `src/render/panels.ts` (R's file): `PanelBody.applyStyle(lowpoly, paint)`.
- `src/main.ts`, `index.html`: style selector, `?style=`, `__wp.style` / `__wp.setStyle`.
- `e2e/run.mjs`: art-style check; `e2e/weather.mjs`: "weather off" compares fog against the
  style's own fog (`__wp.style.baseFogDensity`) instead of 0.
- New: `e2e/art_shots.mjs`, `e2e/art_perf.mjs`.

## Requests
- **Lead (draw-props merge):** in `fleet.ts` keep this branch's `InstancedCar.put` body (it
  applies the style); in `props.ts` set `coneMat` to `PALETTE.propCone` / `propConeGlow` when
  `artStyle.lowpoly` (a 4-line change I can do once props are on my base), and re-`put` props
  on `artStyle.version` change if live switching should recolour parked cars.
- **Lead / perf owner:** `perf_check` thresholds were set on a faster container; here greybox
  fails `framesOver250` (21–29) and sits at the RTF floor. Either a per-machine baseline
  (greybox run as reference) or a looser floor would make the gate meaningful on slow boxes.
- **Testbed / lead:** new views should take colours from `render/palette.ts` and, for lights,
  set `emissiveIntensity` from it when `artStyle.lowpoly` (see `vehicle_view.ts setMeta`).
- **World (05):** a per-building id (or vertex colour) in the greybox would let façade tiers
  vary per building instead of per façade plane; street-lamp positions in the city data would
  replace the kerb-based placement in `style.ts`.
- **Police (06) / lead:** the non-vehicle police units (`police_units()`, helicopters) are still
  boxes; a light-bar mesh for them is a small follow-up on my side once they are drawn properly.

## Known gaps
- Parked props drawn before a live style switch keep their old colours until they move.
- The low profile has no light pools and no bloom (by design, see above).
- Windows ignore real floor heights on hills (grid from absolute height).
- No scrape decals, skid marks or sparks yet (decal pool: prompt 11 later items).
- Light pools are flat discs at kerb height − 13 cm; on steep grades they can clip slightly.
- Switching to greybox after starting in low-poly dark draws greybox without canvas AA (reload
  with `?style=greybox` for the old look exactly).

## Published build
Low-poly dark testbed (ws/art @ 200102b, private, owner): https://claude.ai/artifact/8js9e6J6bjWNFP3Lz6PmTa
— separate from the main testbed link. Built with `vite build --base=./`; the wasm went through
`wasm-opt -Oz` (15.9 → 12.2 MB) to fit the 15 MB per-file limit.

## Lead feedback (round 12, 2026-10-19) — first assignment
The owner picked **stylised low-poly dark** (`docs/art/STYLE_DECISION.md`). Base this round on
`origin/lead/round12` (it holds the newest testbed: weather, panels, minimap, garage), not main.
1. `apps/testbed-web/src/render/palette.ts`: one shared palette (asphalt, kerb, sidewalk,
   building tiers per district, window emissive warm/cool, lamp sodium/LED, car paints, police
   red/blue, fog colour per time of day). Take values from the "low-poly dark" mode in
   `docs/art/style_picker.html`.
2. Apply the look to the city greybox and the car views on the published testbed: flat
   shading everywhere, palette colours, emissive windows/lamps/signals at night, distance fog,
   a light bloom pass on emissives only (own render target + full-screen shader; keep it
   cheap), ACES tonemap. Keep R's panel meshes and their damage (flat-shaded facets are the
   point). Keep weather (K/lead) working on top: wet road darkening and rain must still show.
3. Police light bars are the brightest thing in the frame at night; brake lights and
   headlights read clearly.
4. Performance: measure frame time before/after in the city chase (headless numbers are only
   relative) and keep the post pass ≤ 1 ms on a laptop GPU estimate. Add a debug toggle
   "style: greybox / low-poly dark" so the owner can compare.
5. Screenshots per time of day in `docs/media/art/`; update `docs/art/ART_BIBLE.md` (short).
6. Don't edit `wp_contracts` or Rust sim crates; requests go here.
