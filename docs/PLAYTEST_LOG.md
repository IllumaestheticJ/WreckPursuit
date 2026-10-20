# Playtest log (owner feedback)

## 2026-10-19 — first city chase on the published link
| # | Feedback | Owner of the fix | Status |
|---|---|---|---|
| 1 | No city buildings on the published link | lead (artifact can't serve .glb → base64 fallback in `scene.ts`) | fixed, link v5 |
| 2 | No visible car damage | lead: lattice view on by default; real denting panels = R (prompt 21) | interim fix on link |
| 3 | Cars feel really slow | helper `lead/playtest-feel`: browser sim perf (sim fell behind real time at ~35–50 ms/frame in city chase) + car speed/feel | fixed on `lead/playtest-feel` (numbers below); not yet on the link |
| 4 | Transmission is sloppy | helper `lead/playtest-feel` (shift logic) | fixed on `lead/playtest-feel` (numbers below); not yet on the link |
| 5 | Use real open-source sounds | J (audio): CC0 recorded packs | **done on `ws/audio-samples`**: 80 recorded files (1.25 MB, CC0 + CC-BY 3.0, kenney.nl + opengameart.org) play by default; procedural packs are a fallback setting in the audio panel. Engine recordings are thin (one or two sources per engine type) — details in `docs/integration/audio.md` → "Recorded packs" |
| 6 | Dispatcher and cops are way too talkative | helper `lead/playtest-radio`: `wp_police::radio` director (critical lines always; tactical/routine behind 8 s / 20 s global and 30 s per-unit gaps; duplicates merged; patrol chatter only when wanted or within 200 m; `data/police/radio.toml`) + HUD feed max 3 lines fading after 6 s. 5-min night city chase (`wp_police` scenario `radio_city_chase`, 4 seeds): **in pursuit 30.8 → 5.6 lines/min, not wanted 20.5 → 3.4/min**; calm patrol (`radio_calm_patrol`) 1.0 → 0.1/min. Goldens unchanged (radio not hashed) | fixed on `lead/playtest-radio` |
| 7 | Camera zooms way too far out; need more angles | helper `lead/playtest-camera` | fixed on `lead/playtest-camera`: chase was a world-space spring trailing ≈12 m at chase speed → car-relative near chase 5.6 m back / 1.8 m up (≤1.2 m pull-back; 6.4 m measured at 115 km/h); 8 angles on C / 1–8 (near, far, hood, bumper, cockpit, side, TV, orbit), V hold look back, mouse/stick look-around, remembered per viewer; shots in `docs/media/cameras/` |
| 8 | Higher-power RWD cars: more steering assist / gyro so slides are controllable | helper `lead/playtest-drift`: slide-control assists in `wp_vehicle` (gyro yaw damper, rear-axle countersteer, slide throttle), keyboard/gamepad only | done on branch |
| 9 | "Give some relevant cars a little more steering angle to save the car" | helper `lead/playtest-drift`: lock opens while sliding at speed; more lock on 5 drift-capable RWD cars (shiro_turbo 39.8°, sperber_32 38.1°, rampart_50 37.9°, hakone_gt 35.9°, lucciola_350 33.1°) | done on branch |
| 10 | Arcade damage too aggressive: permanent damage after one hit | B (damage): cosmetic vs mechanical split in Arcade; light/medium hits dent but no lasting handling effects | **done on `ws/damage`** (bf99fc0 + [bless] 10ae2b6): Arcade hits closing < ~41 km/h are cosmetic (dent + bill, no handling/component loss); hard hits unchanged, 30 % of handling loss settles back over 20 s; three hard hits wreck the car (health ≈ 0.2; Arcade total-loss line 0.23 waits on G's career model). Before/after in `docs/integration/damage.md`. Sim unchanged |
| 11 | Cop cars have no regard to traffic | E (police): obstacle awareness via D leader query; patrol obeys traffic, pursuit avoids civilians | fixed on `ws/police` (round 6): units follow/wait/pass, stop at reds, give way, careful junctions with lights. 0 police↔traffic contacts in 5 min patrol and 6 min hot pursuit (`police_traffic_contacts` example); see `docs/integration/police.md` |
| 12 | Cop cars too fragile | H (cars): sentinel_cruiser police package — durability 5, 40 kg push bar (30 kN·s), bumper fixings ×2, HD cooling/suspension/steering, +40 kg on the nose; B arcade split helps too | done on `ws/cars` (H): full-sim ram sequence (`tools/specsheet/ram_sequence.py`) — Arcade: 5 rams/PITs at 55-70 km/h + 2 hits at 85 km/h, never a total loss, still in pursuit (health 0.42) after the 1st hard hit; Sim: survives 4 rams (civilian sedan: 2). Numbers in `docs/integration/cars.md` |
| 13 | Road flickers a lot | lead: z-fighting (debug grid + coplanar greybox layers) — grid hidden in city, debug ground lowered, polygon offset per road layer, camera near/far 0.1/3000 | fixed, link v6 |
| 14 | NPC cars need variance: lane changes, dynamic driving, different speeds | D (world): per-driver personality, MOBIL lane changes | **done on `ws/world`** (awaiting merge): per-driver speed 0.80–1.25× limit (p5 0.84 / p95 1.13; highways faster, alleys slower), headway/accel/brake ranges, reaction delay at greens (8 % hesitant), casual amber runners, per-driver overtaking eagerness, indicator ≥ 0.5 s before lane changes; 0 civilian collisions, perf unchanged — numbers in `docs/integration/world.md` |
| 15 | City needs to be more dynamic: curves and elevation | helper `lead/city-v2`: Port Vesper-style procedural city, heightfield, curved/hill roads, terrain colliders | done on `lead/city-v2` (not merged): Port Vesper v2 is the city-chase default (selector keeps the grid) — downtown grid, organic districts, Hollins Drive switchbacks + Ridge Road crests (≈ 110 m hill), banked Loop 9 with an overpass and an underpass; terrain/road/building colliders; hill and loop scenarios at the limit pass (on road, never airborne); screenshots in `docs/media/city-v2/` |
| 16 | Traffic lights not visible | helper `lead/playtest-signals`: `signal_heads()`/`signal_states()` wasm exports; mast-arm + far-side heads, stop lines, night glow (layer key 6); screenshots `docs/media/signals/` | fixed on `lead/playtest-signals` (not on the link yet) |
| 17 | (lead helper, city v2) Port Vesper v2 needs a playtest on the published link: hills at speed, the Loop crest, the overpass/underpass, buildings now solid | owner → lead | to playtest (`?city=procedural_12x12` keeps the old grid) |

### Notes (moved below the table)
Numbers for 8–9 (`cargo run --release -p wp_vehicle --example slide`; scripted naive driver, worst body slip = max yaw excursion from the direction of travel, recover rate = cases that end straight and moving):
| Scenario | Preset | main | lead/playtest-drift |
|---|---|---|---|
| Power-oversteer exit, rampart_50 (319 kW RWD, 2nd gear) | keyboard, TCS/ESC off | 180° (spins), 0/4 | 29°, 6/6 |
| Power-oversteer exit, lucciola_350 (207 W/kg) | keyboard, TCS/ESC off | 180°, 0/4 | 29°, 6/6 |
| Power-oversteer exit, rampart / lucciola / sperber / hakone | keyboard, TCS/ESC off | 1/16 recovered | 24/24 recovered |
| Power-oversteer exit, rampart_50 | gamepad | 71°, 2/4 | 13°, 6/6 |
| Power-oversteer exit, rampart_50 | keyboard (TCS on) | 9°, 4/4 | 6°, 6/6 |
| Catch at 110/130 km/h, rampart / shiro / lucciola | keyboard, TCS/ESC off | 180°, 7/12 recovered | ≤ 7°, 12/12 |
| Any of the above | wheel (help 0) | spins | bit-identical to main |
### Row 6 gate notes (`lead/playtest-radio`)
- fmt, `clippy --workspace --all-targets -D warnings`, `lint-determinism`, wasm32 build, ui-kit + testbed `tsc`, ui-kit vitest: pass.
- `cargo test --workspace`: everything passes except `simcli goldens_match`. Eight police-city goldens
  (city_chase, night_pursuit_escape, opponent ×4, race_city_sprint, traffic_soak) diverge at tick 24.
  This comes from the required merge of `origin/ws/police` (round 6 obstacle awareness): `origin/main` passes;
  the merge commit *before* any radio change fails with the same hashes as after it, so the radio change
  hashes the same. Nothing re-blessed here; E or the lead blesses with the police merge.
- `web-e2e`: everything passes except "city chase … speeding violations in the feed" (heat stays 0). It fails
  the same way on the merge commit without the radio change.
### Rows 3–4: before / after (`lead/playtest-feel`, 2026-10-19, cloud container)
**Row 3 — slow motion.** Cause: frames slower than 100 ms lost sim time (the frame loop capped
catch-up at 0.1 s), and the city chase cost more per tick than it needed. Fixes: 0.25 s
catch-up; 3,300 city props fixed until a car comes near; no per-tick police car clone; faster
(same-value) replay hash; lattice views rebuilt only when dented. Determinism unchanged (all
35 scenario hashes identical across the perf commit; native == wasm 35/35).
| City chase (debug_sedan, Arcade, W held) | Before | After |
| Native sim, ms per 60 Hz frame: mean / p95 / worst | 3.1 / 4.8 / 73 | **1.2 / 2.2 / 4.6** |
| — Rapier (props) / police / replay hash / vehicles | 0.92 / 0.41 / 0.83 / 0.51 | 0.15 / 0.01 / 0.40 / 0.34 |
| — first 0.5 s of a chase | 23 ms per frame (props awake) | no spike |
| Browser (wasm) sim, ms per 60 Hz frame | 3.5–4.2 | **2.6–3.1** |
| Browser JS views (lattice/skin), ms per frame | 16.4 | **4.8–5.3** |
| Real-time factor, headless 1280×720 (CPU raster, 3–6 fps) | **0.35** | **0.90–0.94** |
| Real-time factor, headless 480×270 | — | 0.96–0.99 |
Headless Chromium rasterises on the CPU (SwiftShader), so its fps is not the owner's; with a
GPU the JS work per frame is ~2.8 (sim) + ~5 (views) + ~5 (render submit) ms. The HUD now
shows `real time 1.00×` and an amber "⚠ slow motion" warning below 0.95×.
**Row 3 — the car.** City chase / free drive default to **Lindwurm 4T** (AWD, 80 ms dual-clutch,
0–100 5.4 s, in its 4.6–5.4 band) instead of the debug sedan (8.3 s). Keyboard throttle: JS
ramp 0.2 → 0.08 s and the sim's keyboard filter 0.44 → 0.23 s (they were stacked).
0–100 km/h (`wp_vehicle` metrics, release; TCS on; band from `data/cars/VALIDATION.md`):
| Car | Band | Before | After |
| Lucciola 350 | 4.3–5.0 | 4.57 | 4.55 |
| Debug Coupe | (5–6) | 5.23 | 5.20 |
| Lindwurm 4T (new default) | 4.6–5.4 | 5.40 | 5.42 |
| Sentinel Wolfhound | 5.5–6.5 | 5.55 | 5.58 |
| Rampart 5.0 | 4.5–5.0 | 5.93 ✗ | 5.84 ✗ |
| Hakone GT | 4.8–5.8 | 5.84 ✗ | 5.86 ✗ |
| Sperber 32 | 5.0–5.8 | 6.06 ✗ | 6.05 ✗ |
| Shiro Turbo | 5.6–6.6 | 6.30 | 6.42 |
| Kestrel TS | 6.2–7.2 | 7.14 | 7.08 |
| Debug Sedan (old default) | (8–10) | 8.13 | 8.29 |
| Vireo XS | 8.5–9.5 | 9.20 | 9.45 |
Launches are traction/TCS-limited (TCS off is slower for every 2WD car), so the gearbox barely
moves WOT times. The old numbers carried a small unphysical bonus: the old box let the engine
flare to the limiter in every shift and then dumped the clutch (worth ~4 km/h per shift on the
Vireo). The three ✗ cars were already out of band (H's VALIDATION notes: TCS-conservative).
**Row 4 — sloppy transmission.** Old automatic: drive cut for the whole shift with the clutch
open, engine flared to the limiter (6,772 rpm on the sedan, every shift), then a 0.1 s clutch
re-ramp; at 35% pedal it still revved to 88% of redline before upshifting; at half throttle
the schedule could hunt (1→2→1). New automatic (`crates/wp_vehicle/README.md`):
| Check | Before | After |
| Shift (sedan / Hakone, 0.25 s) | full cut 0.25 s + 0.1 s clutch ramp, engine flare to limiter | release → rev-matched neutral (0.1 s no drive) → in-sync re-engage; engine 6,500 → 3,070 rpm before the clutch closes |
| Hunting at constant pedal | possible (sedan at 50% pedal: the 1→2 upshift landed below the downshift point) | none at any pedal 0–1 / any speed (unit test) and over a 0.2–1.0 throttle sweep on 4 cars (rig test) |
| Upshift point at 35% pedal | 88% of redline (5,700 rpm sedan) | ~57% (load-based; 0.36 light … 0.975 full) |
| Kickdown | one gear when rpm fell below the point | pedal stab → lowest gear ≤ 85% redline, skipping gears (≥ 2 gears in one shift from a 70 km/h cruise, tested) |
| Mid-corner at the limit | shifts | held (lateral usage ≥ 0.75) |
| Braking | one gear per 0.65 s | sequential, never above 72% redline, held 1.5 s after the brake |
| Standard launch (WOT) | — | sedan 2 upshifts by 10 s, Hakone 2 by 8 s, 0 downshifts (tests) |

## 2026-10-19 — greybox demo end of run (lead helper `lead/demo-endrun`)
| # | Item | Owner | Status |
|---|---|---|---|
| 18 | Captured but nothing happens; no fine / impound / bounty; sirens not tied to the chase | lead (`wp_wasm::run`, testbed `src/ui/runEnd.tsx`) | done on `lead/demo-endrun`: capture / wreck holds the car, opens the result panel (fine, bail, impound fee, G's repair quote, cash before → after, ledger lines) + the bill; **Repair & continue** pays from the ledger and restores the car; escape → bounty toast; sirens/light bars follow unit assignment. Goldens unchanged. To playtest on the link |
| 19 | Engine sound from 2nd gear onwards doesn't work well | J (audio): reproduce with dumped rpm/gear/load/layer signals after the new gearbox; fix and add regression tests | **fixed on `ws/audio`**: the browser voice pool dropped every update < 0.4 %/frame, so from 2nd gear (slower rpm climb) the engine pitch and layer gains froze at the last shift (4th gear: note stuck at 155 Hz while the crank went 162 → 210 Hz); procedural layers also cancelled each other in crossfades (up to 10 dB). Now one engine synth per car that tracks rpm within 0.6 % in every gear (was 28–32 % off), level steady within 0.4–0.9 dB (was 3.5–9.6 dB), plus a shift note (applied-throttle cut + rpm-rate dip 2–5 dB and re-rise, blips on rev-matched downshifts, blow-off on turbo upshifts) and on-load part throttle. 5 cars, 0 clicks before/after. Details + tests in `docs/integration/audio.md` |
| 20 | Car can fall into / through the sidewalk | helper `lead/fix-sidewalk-fall`: match colliders to the visual sidewalk/kerb surface (Port Vesper v2), kerb steps, seam sweep test | dispatched |
| 21 | Need a game map HUD with police | M (UI kit): rotating minimap with police dots, sight cones and search zone, plus a full city map (M/Tab) | done on `ws/ui`: heading-up minimap (speed zoom, police blink red/blue when responding/pursuing, steady on patrol, sight areas + helicopter spotlight while wanted, search zone + escape ring while searching, roadblocks/spikes, garages/safehouses) and a full city map on **M** / **Tab** (districts named, roads by class, overpasses on top, POIs, pan/zoom, legend; driving input paused). Mute moved to **V**. Screenshots `docs/media/map/`. To playtest on the link |
| 22 | Brake lights stopped working | lead: the panel meshes hide their own light parts inside the shell; lamp markers kept on, placed at the panel body nose/tail (R to fix light parts in the mesh generator) | fixed on link v12 |
| 23 | Keys get stuck (a key stays "held") / buttons re-trigger | lead: release all keys on tab switch, context menu, focus into form controls, Cmd (macOS); clicked buttons give focus back, so Space/Enter no longer re-press them | fixed on link v12 |
| 24 | Sideswiped a cop and no chase started | E (police): hitting a police car is now assaulting an officer (`[heat.assault]` in `data/police/heat.toml`). A sideswipe at moderate speed (relative speed ≥ 2.5 m/s, impulse ≥ 150 N·s, racer moving) raises heat to ≥ L2 and starts the pursuit in the same tick, day or night, with the hit unit (no 4 s recognition wait); the radio says "Unit N: I've been hit! … assault on an officer." and the PURSUIT banner shows. A 5 km/h parking bump does nothing. Tested with real cars on the grid and Port Vesper v2, day and night (`wp_sim/tests/police_assault.rs`), police scenarios and simcli `police/sideswipe_patrol_{day,night}` | fixed on `ws/police`, to playtest |
| 25 | Some cars load their LOD too slowly (pop-in) | helper `lead/fix-lod-popin` (C testbed meshes + D traffic LOD) | dispatched |
| 26 | Simulate LSDs in certain cars | A (vehicle) + H (car data) | **done on `ws/vehicle`** (A): every car has a real diff now — open, clutch LSD 1 / 1.5 / 2-way, viscous, helical, active e-diff, welded (+ AWD centre) — set per car with a reason in `data/cars/differentials.toml` (H owns it from here). Same RWD coupe (`shiro_turbo`), open → 1.5-way clutch LSD: full-throttle exit from a tight corner (slide help on) **inside-wheel slip 0.97 → 0.21, exit speed 52 → 58 km/h**, more oversteer but caught (6° → 12° body slip); one side on ice at 60 % throttle **4 m → 11 m** in 4 s. Lifting mid-corner, a 1-way tucks in more than a 2-way (curvature ×1.069 vs ×1.052). Open-diff cars now get brake-based traction control: ice/tarmac pull-away with TCS on, hatch 3 → 18 m, police SUV 2 → 19 m, AWD hatch 5 → 35 m in 4 s. Accel/skidpad stay in their bands (AWD pursuit sedan 0-100 5.5 → 5.9 s). Numbers: `cargo run --release -p wp_vehicle --example diff_demo`; details `docs/integration/vehicle.md` |
| 27 | Audio for wheelspin | J (audio) | **done on `ws/audio`**: own `tire/spin` loop (gravel: `tire/spin_loose`) from per-wheel driving slip ratio × slip speed, weighted by load and surface, pitch/intensity rising with slip speed; two spinning wheels louder than a one-wheel peel; squeal/skid no longer fire for straight-line spin. Burnout (V8, TCS off): spin 0.41–0.46, squeal 0 (was squeal 0.42, no spin); silent within 0.2 s of grip. Regression test `crates/wp_audio/tests/wheelspin.rs`; details in `docs/integration/audio.md` |
| 28 | All cars understeer too much | helper `lead/handling-balance`: measure understeer per car and assist profile (lock reduction, steer filter, ESC, tyres, setup), fix at the root, keep the wheel profile raw | dispatched |
| 24 | Sideswiped a cop and no chase started | E (police) | dispatched |
| 25 | Some cars load their LOD too slowly (pop-in) | helper `lead/fix-lod-popin` (C testbed meshes + D traffic LOD) | done on `lead/fix-lod-popin` (not merged): all four causes were real. Car meshes (player, traffic, police) preload at session start and build on a car's first frame (was: fetched when a model first appeared, placeholder hull until then); kinematic traffic and unbound police are instanced cars of the real model and livery (was: grey boxes), and promotion / demotion swaps to the full car on the same frame with the same pose (e2e: 0 frames without a mesh, swap jump < 0.5 m; wasm test: worst 0.06 m over 15 swaps); traffic spawns / despawns ≥ 320 m from the player (was 150 m; measured pop-in 226 m → 359 m). `wp_world` radii changed: goldens blessed. Strips + frame logs in `docs/media/lod/`. To playtest on the link |
| 26 | Simulate LSDs in certain cars | A (vehicle) + H (car data) | dispatched |
| 27 | Audio for wheelspin | J (audio) | dispatched |
| 28 | All cars understeer too much | helper `lead/handling-balance`: measure understeer per car and assist profile (lock reduction, steer filter, ESC, tyres, setup), fix at the root, keep the wheel profile raw | done on `lead/handling-balance` (numbers below): chassis was already near neutral; keyboard/gamepad lock was ~3× too big at speed (fronts washed out) and ESC braked in ordinary corners. Held key now 90–100 % of grip (was 70–92 %). Wheel preset unchanged. One test outside `wp_vehicle` fails on this branch (wp_net `city_e2e`, damage replica invariant exposed by the new crash sequence), see `docs/integration/vehicle.md` |
| 29 | TCS / ESC / automatic gears off by default | lead: the testbed starts with TCS, ESC and auto gears off (keyboard and gamepad); any assist box you change is remembered | fixed on link v13 |
| 30 | Siren audio error broke the frame loop (found by the e2e after row 29) | lead: siren pitch curves no longer overlap (float rounding); a bad curve can no longer stop the page | fixed on link v13 |
| 31 | Hood does not crumple; headlights stay in place in crashes | R (panels): hood skinning; light parts on the surface and moving with the crush (then the lead drops the lamp markers) | **done on `ws/panels`** (R round 2): hood/trunk lids buckle (fold from the crushed hinge→latch chords: 56 km/h frontal → 0.22 m peak, creases, raised rear edge when unlatched); light lenses on the outer surface, skinned to bumper/hood/fender/frame, crush with the nose and break (dark) when crushed 5 cm or their bumper hangs; lenses were rendering black (no normals) — fixed, brake glow on the lenses, lamp markers removed from panel bodies. Shots `docs/media/panels/{before_frontal_56_hood,frontal_56_hood_buckled,before_brake_lights,brake_lights_on}.png` |
| 32 | AI cars, when blocked by a stationary vehicle, should go around it or turn around | D (traffic AI) | **done on `ws/world`** (round 9): a car held up by a wreck, parked player, stopped police car or debris (not a red light or normal queue) waits its patience (3–8 s by personality), then changes lane (multi-lane), passes slowly in the oncoming lane when clear (no passing near junctions, over crests or round bends), or turns around (U-turn / k-point turn) or reverses to the last junction and re-routes; kinematic and physical cars. Parked-player scenarios (grid + Port Vesper, 5 seeds each): 0 civilian collisions, parked car never touched, nobody stuck; 400 agents 0.091 ms/frame with 12 blockers. Police patrols: helpers + request to E in `docs/integration/world.md` |
| 33 | Step 2: weather in the playable game (rain, wet roads, fog, selector, rainy-night chase) | lead: `lead/weather-in-game` — weather selector + **rainy night** button, wet grip / aquaplaning / fog-limited police sight in the sim; try braking and cornering in heavy rain and losing the police in fog | on branch, not on the link yet |
| 34 | Step 2: a garage and a career that persist between runs | G (economy): `wp_wasm::career` + `wp_economy::profile`, garage screen `apps/ui-kit/src/garage/`, testbed `garage` button + `career` box; cash, ledger, cars with saved dents and mods, pound, insurance, heat carry-over in `localStorage` (export / import as a file); free drive and city chase use the career car; new players start with 4 500 (a worn beater + three busted nights), escapes pay cash | done on `ws/economy` (awaiting merge); native tests `wp_wasm::career_tests`, browser e2e "career:" lines; numbers in `docs/integration/economy.md` |
| 42 | (found by J) With TCS off the automatic box hunts 1↔2 during a burnout / wheelspin launch (rampart_50) | helper `lead/handling-balance` (shift logic; A for the diff side) | open |
| 43 | Economy exploit: buy a used car and sell it for more | G (economy): resale from the purchase condition, property tests | dispatched |
| 44 | Police spawn too far away; spawn them in response to reckless-driving calls | E (police): dispatch-driven spawning out of sight, 10–25 s response | dispatched |
| 45 | Still falling into sidewalks (on the round-12 preview) | helper `lead/fix-sidewalk-fall` (merged into `lead/round12` 2026-10-20, after the preview was built) | fixed on `lead/round12`; next preview / link |
| 46 | Intersections still have holes in them (Port Vesper v2) | helper `lead/fix-junction-holes` (wp_world surface junction discs / bend connectors, greybox + colliders) | dispatched |
| 47 | Some cars are missing sounds (round-12 preview) | J (audio): find which cars/voices are silent; suspect the lead's J + J2 merge of `voices.ts` (resolve() now runs before the engine-instrument check) and per-car pack mapping | dispatched |
| 48 | Police cars are too fragile | B (damage) + H (police package): Arcade wreck thresholds, police ≥ player sports car | dispatched |
| 49 | Cars in general are too fragile in Arcade | B (damage): retune `data/modes/arcade.toml` to "punishing but survivable" | dispatched |
| 50 | Hitting invisible objects (wrecks on an empty road) | lead: parked-car and cone props are physical but never drawn → helper `lead/draw-props` | dispatched |
| 51 | The painted stop lines at traffic lights make the car jump and crash into the light | helper `lead/fix-junction-holes` (step in the collision surface where the lane meets the junction disc) | dispatched |
| 52 | The sidewalk should be at the same height as the base of the buildings | helper `lead/fix-junction-holes` (building base meets the sidewalk top along frontages; plinths on slopes) | dispatched |
| 53 | Hood doesn't crumple enough | R (panels): stronger visual buckling (fold lines, tenting, latch kink), visual only | dispatched |
| 54 | Cars are too bouncy / light and get launched too easily | A (vehicle): damping, bump stops, inertia, restitution, kerb-strike impulses; arcade anti-launch assist | dispatched |
| 55 | Police don't respond / spawn close enough, or get lost / damaged on the way | E (police): per-unit response log (spawn distance, route, failure cause), re-route / respawn out of sight; durability = B row 48 | dispatched (with row 44) |
| 56 | Semi-auto (and auto): holding the handbrake also opens the clutch so the rear wheels lock | A (vehicle): auto-clutch decouples while the handbrake is held, smooth re-engage, no stall; both modes | dispatched |
| 57 | Countersteer assist as a slider with variable aggression | A (vehicle): 0–100 % slide-catch gain (driver override kept), wasm + testbed slider | dispatched |
| 58 | Horn should sound for as long as the button is held | J (audio): looped horn sustain (recorded + procedural), release ≤ 50 ms | dispatched |
| 59 | No option to map a clutch | C (testbed): `clutch` binding (key / pad / wheel pedal) + automatic/manual clutch setting; A: manual clutch + stall/restart in wp_vehicle | dispatched |

## 2026-10-19 — step 2: 4-player test on Port Vesper v2 (F, `ws/net`)
Found by the new 3-minute 4-client test (`crates/wp_net/tests/vesper_e2e.rs`) and the
two-tab browser check (`just web-e2e-net`).
| # | Item | Owner | Status |
|---|---|---|---|
| 35 | A player cop could not start a pursuit or capture anyone: player police seats were not `wp_police` units | F (+ glue `Sim::bind_player_police`) | fixed on `ws/net`: the cop spots racer 0 and starts the pursuit, takes the captain's order, calls in a roadblock + spikes, captures; all 4 clients see the capture |
| 36 | Police never recognised a racer in downtown Port Vesper: sightlines at a fixed 1.3 m ran through the terrain (downtown is ~5 m up) | F fix in `wp_police::sees_at` (E to review) | fixed on `ws/net`; identical on flat cities (z < 1 m) |
| 37 | Multiplayer on Port Vesper: own-car prediction drove on the flat slab while the server drives on terrain | F (`Rig::set_city_ground` with `wp_sim::city_ground_colliders`) | fixed: RMS correction 0.03–0.12 m on terrain |
| 38 | Another player's car could freeze on one client for good (its first deltas arrived before `EntityEnter`; baseline deadlock) | F (protocol v5 `Inputs.missing`) | fixed: 0 missing baselines in 3 min |
| 39 | Damaged cars entering view diverged on the next hit (positions-only snapshot, NET-N5) | F (full `DamageSnapshot` in lattice snapshots, provisional follow-up) | fixed: 0 / 3844 lattice hash mismatches (was 106 / 1345 in the first run) |
| 40 | Roadblocks / spikes are never possible in downtown Port Vesper (a side street every 100 m breaks the 120 m sight rule) | E (police) design question | open |
| 41 | Two browser tabs: join, see each other, crash, see each other's dents | owner | to playtest: `just server`, then `net.html?role=racer` in two tabs (the e2e passes) |
| 31 | Hood does not crumple; headlights stay in place in crashes | R (panels): hood skinning; light parts on the surface and moving with the crush (then the lead drops the lamp markers) | dispatched |
| 32 | AI cars, when blocked by a stationary vehicle, should go around it or turn around | D (traffic AI) | dispatched |
| 33 | Step 2: make street races playable in the browser, like the city chase | L (race) + glue in `wp_wasm` / testbed | done on `ws/race`: **races** button → picker (Hollins Drive hill sprint, Loop 9 circuit, Lantern Row illegal-daytime checkpoint run, grid highway sprint) with 3–5 named AI racers, countdown + grid hold, race HUD (position, lap/CP, gaps, standings, minimap, next-gate arch), results with G's payout, police capture → end-of-run panel. Details in `docs/integration/race.md` (Step 2) |
Numbers for 28 (`cargo run --release -p wp_vehicle --example handling`; `before` = `ws/vehicle` a4c054c (main + A's differentials, row 26), 80 km/h unless noted; δ = road-wheel angle; "limit" = the car's max g, wheel preset). Causes, method and the failing tests: `docs/integration/vehicle.md` → "Handling balance".
| Car | Class | US gradient deg/g, keyboard (wheel = chassis) | δ at 0.6 g, deg: keyboard taps, R80 | Input at 0.6 g: keyboard / gamepad | Held key g at 80 km/h (limit) | Full stick g (gamepad) | Turn-in rise s: full key / gamepad 0.4 g | City corner ESC share (keyboard) |
|---|---|---|---|---|---|---|---|---|
| vireo_xs | Compact FWD | 0.49 → **0.46** (0.46) | 3.55 → **2.93** | 0.10 → **0.19** / 0.07 → **0.17** | 0.79 → **0.85** (0.87) | 0.78 → **0.85** | 0.17 → **0.15** / 0.20 → **0.20** | 0.57 → **0.07** |
| kestrel_ts | Sport FWD | 0.36 → **0.37** (0.35) | 3.24 → **3.25** | 0.11 → **0.22** / 0.08 → **0.19** | 0.84 → **0.91** (0.93) | 0.83 → **0.90** | 0.16 → **0.15** / 0.20 → **0.20** | 0.58 → **0.07** |
| shiro_turbo | Sport RWD | 0.39 → **0.34** (0.34) | 3.29 → **2.80** | 0.09 → **0.21** / 0.07 → **0.19** | 0.75 → **0.91** (0.95) | 0.72 → **0.90** | 0.13 → **0.15** / 0.19 → **0.19** | 0.58 → **0.23** |
| hakone_gt | Sport RWD | 0.34 → **0.18** (0.18) | 3.15 → **2.80** | 0.10 → **0.20** / 0.07 → **0.18** | 0.74 → **0.87** (0.92) | 0.71 → **0.86** | 0.13 → **0.15** / 0.21 → **0.21** | 0.62 → **0.09** |
| rampart_50 | Muscle RWD | 0.32 → **0.32** (0.32) | 3.60 → **3.25** | 0.10 → **0.22** / 0.08 → **0.20** | 0.71 → **0.84** (0.88) | 0.68 → **0.83** | 0.13 → **0.15** / 0.21 → **0.20** | 0.53 → **0.22** |
| ironside_440 | Muscle RWD | 1.77 → **1.77** (1.77) | — → **—** | 0.25 → **0.45** / 0.20 → **0.38** | 0.43 → **0.55** (0.61) | 0.40 → **0.53** | 0.17 → **0.17** / 0.32 → **0.31** | 0.59 → **0.51** |
| sperber_32 | Sport RWD | 0.50 → **0.21** (0.21) | 3.12 → **2.76** | 0.10 → **0.20** / 0.07 → **0.18** | 0.74 → **0.89** (0.94) | 0.71 → **0.88** | 0.12 → **0.14** / 0.18 → **0.17** | 0.61 → **0.09** |
| lindwurm_4t | Sport AWD | 0.40 → **0.33** (0.33) | 3.34 → **2.85** | 0.11 → **0.21** / 0.08 → **0.19** | 0.83 → **0.90** (0.94) | 0.82 → **0.89** | 0.14 → **0.15** / 0.18 → **0.18** | 0.63 → **0.14** |
| lucciola_350 | Exotic RWD | 0.18 → **0.07** (0.07) | 3.28 → **2.65** | 0.10 → **0.19** / 0.07 → **0.17** | 0.78 → **0.93** (0.99) | 0.76 → **0.91** | 0.11 → **0.13** / 0.18 → **0.18** | 0.66 → **0.15** |
| hauler_1500 | Truck RWD | 1.74 → **1.74** (1.74) | 5.57 → **6.69** | 0.19 → **0.29** / 0.17 → **0.25** | 0.57 → **0.62** (0.64) | 0.55 → **0.60** | 0.19 → **0.18** / 0.28 → **0.28** | 0.41 → **0.33** |
| sentinel_cruiser | PoliceCruiser RWD | 0.43 → **0.35** (0.35) | 3.65 → **3.58** | 0.12 → **0.22** / 0.09 → **0.20** | 0.70 → **0.81** (0.85) | 0.68 → **0.80** | 0.16 → **0.16** / 0.21 → **0.21** | 0.52 → **0.04** |
| sentinel_wolfhound | PoliceInterceptor AWD | 0.41 → **0.32** (0.32) | 3.38 → **3.28** | 0.12 → **0.21** / 0.09 → **0.19** | 0.77 → **0.82** (0.85) | 0.76 → **0.82** | 0.17 → **0.16** / 0.24 → **0.23** | 0.50 → **0.08** |
| sentinel_bastion | PoliceSuv RWD | 0.64 → **0.49** (0.49) | 3.91 → **3.93** | 0.13 → **0.23** / 0.10 → **0.21** | 0.62 → **0.70** (0.72) | 0.60 → **0.68** | 0.15 → **0.14** / 0.23 → **0.23** | 0.44 → **0.08** |
| junkyard_beater | Beater FWD | 1.42 → **1.42** (1.42) | — → **3.38** | 0.11 → **0.24** / 0.09 → **0.21** | 0.75 → **0.82** (0.83) | 0.73 → **0.81** | 0.18 → **0.15** / 0.18 → **0.18** | 0.52 → **0.48** |
| tansy_13 | Compact FWD | 0.64 → **0.63** (0.63) | — → **3.08** | 0.10 → **0.20** / 0.08 → **0.18** | 0.73 → **0.79** (0.79) | 0.72 → **0.78** | 0.17 → **0.14** / 0.18 → **0.18** | 0.57 → **0.11** |
| drayman_cargo | Truck RWD | 0.78 → **0.78** (0.78) | 5.69 → **5.67** | 0.14 → **0.28** / 0.12 → **0.25** | 0.57 → **0.63** (0.66) | 0.54 → **0.62** | 0.16 → **0.15** / 0.23 → **0.23** | 0.39 → **0.33** |
| furrow_24 | Truck RWD | 0.57 → **0.54** (0.54) | 3.93 → **3.58** | 0.12 → **0.22** / 0.09 → **0.20** | 0.62 → **0.70** (0.73) | 0.60 → **0.69** | 0.16 → **0.15** / 0.22 → **0.22** | 0.40 → **0.14** |
| 19 | Engine sound from 2nd gear onwards doesn't work well | J (audio): reproduce with dumped rpm/gear/load/layer signals after the new gearbox; fix and add regression tests | dispatched |
| 20 | Car can fall into / through the sidewalk | helper `lead/fix-sidewalk-fall`: match colliders to the visual sidewalk/kerb surface (Port Vesper v2), kerb steps, seam sweep test | **done on `lead/fix-sidewalk-fall`** (+ [bless]): sidewalks had **no collider** — wheels sank 0.27 m (Vesper; 0.15 m grid) and on fills/deck approaches fell metres; junction discs didn't reach the road ribbons' corners (unpaved crescents at every junction mouth); pits at junction corners/bends. Now one shared mesh for greybox + physics (sidewalks, 0.15 m kerbs, corner fills, full junction discs); visual vs physics sweep 294 k → 0 samples off; 96 kerb mounts at 15/40 km/h, worst tire −2.3 cm (was −34 cm). Details below and `docs/integration/world.md`. To playtest on the link |
### Row 20 detail — sidewalk fall (lead helper `lead/fix-sidewalk-fall`)
| What was wrong | Where | Fix |
| Sidewalks/kerbs drawn, never collided: physics under them was terrain 12 cm below the road (Vesper) or the slab (grid) | every street | sidewalk tops + kerb faces as colliders (`wp_world::surface::sidewalk_mesh`, same triangles as the greybox) |
| Sidewalk on fills / deck approaches floated over natural ground up to 21 m below | Vesper embankments, ramps | same (collider at the visual height) |
| Junction disc smaller than the ribbon corners: unpaved crescents ≤ 1.7 m in each junction mouth | every junction | paved disc reaches every ribbon corner, polygon encloses its circle |
| No sidewalk between streets at corners / outside bends (15–27 cm pits; the next sidewalk's edge stopped cars) | junction corners, bends | corner / T / bend fills at kerb height |
| Chassis box (floor at kerb height) snagged on the kerb while straddling it | anywhere | upright cars: chassis ignores sidewalks, the wheels climb |
