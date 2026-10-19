# Playtest log (owner feedback)

## 2026-10-19 — first city chase on the published link
| # | Feedback | Owner of the fix | Status |
|---|---|---|---|
| 1 | No city buildings on the published link | lead (artifact can't serve .glb → base64 fallback in `scene.ts`) | fixed, link v5 |
| 2 | No visible car damage | lead: lattice view on by default; real denting panels = R (prompt 21) | interim fix on link |
| 3 | Cars feel really slow | helper `lead/playtest-feel`: browser sim perf (sim fell behind real time at ~35–50 ms/frame in city chase) + car speed/feel | dispatched |
| 4 | Transmission is sloppy | helper `lead/playtest-feel` (shift logic) | dispatched |
| 5 | Use real open-source sounds | J (audio): CC0 recorded packs | **blocked**: container network policy refuses kenney.nl / opengameart.org / freesound.org / sonniss.com etc. — hosts to allow listed in `docs/integration/audio.md`; no audio committed |
| 6 | Dispatcher and cops are way too talkative | helper `lead/playtest-radio` | dispatched |
| 7 | Camera zooms way too far out; need more angles | helper `lead/playtest-camera` | dispatched |
| 8 | High-power RWD cars: more steering assist ("gyro") to control slides | helper `lead/playtest-drift` | dispatched |
| 9 | More steering angle on relevant cars to save slides | helper `lead/playtest-drift` (lock opens while sliding + per-car `max_steer_rad`) | dispatched |
| 10 | Arcade damage too aggressive: permanent damage after one hit | B (damage): cosmetic vs mechanical split in Arcade; light/medium hits dent but no lasting handling effects | dispatched (follow-up to B round 6) |
| 11 | Cop cars have no regard to traffic | E (police): obstacle awareness via D leader query; patrol obeys traffic, pursuit avoids civilians | fixed on `ws/police` (round 6): units follow/wait/pass, stop at reds, give way, careful junctions with lights. 0 police↔traffic contacts in 5 min patrol and 6 min hot pursuit (`police_traffic_contacts` example); see `docs/integration/police.md` |
| 10 | Arcade damage too aggressive: permanent damage after one hit | B (damage): cosmetic vs mechanical split in Arcade; light/medium hits dent but no lasting handling effects | **done on `ws/damage`** (bf99fc0 + [bless] 10ae2b6): Arcade hits closing < ~41 km/h are cosmetic (dent + bill, no handling/component loss); hard hits unchanged, 30 % of handling loss settles back over 20 s; three hard hits wreck the car (health ≈ 0.2; Arcade total-loss line 0.23 waits on G's career model). Before/after in `docs/integration/damage.md`. Sim unchanged |
| 11 | Cop cars have no regard to traffic | E (police): obstacle awareness via D leader query; patrol obeys traffic, pursuit avoids civilians | dispatched (E round 6 priority) |
| 12 | Cop cars too fragile | H (cars): sentinel_cruiser police package — durability 5, 40 kg push bar (30 kN·s), bumper fixings ×2, HD cooling/suspension/steering, +40 kg on the nose; B arcade split helps too | done on `ws/cars` (H): full-sim ram sequence (`tools/specsheet/ram_sequence.py`) — Arcade: 5 rams/PITs at 55-70 km/h + 2 hits at 85 km/h, never a total loss, still in pursuit (health 0.42) after the 1st hard hit; Sim: survives 4 rams (civilian sedan: 2). Numbers in `docs/integration/cars.md` |
| 13 | Road flickers a lot | lead: z-fighting (debug grid + coplanar greybox layers) — grid hidden in city, debug ground lowered, polygon offset per road layer, camera near/far 0.1/3000 | fixed, link v6 |
| 14 | NPC cars need variance: lane changes, dynamic driving, different speeds | D (world): per-driver personality, MOBIL lane changes | **done on `ws/world`** (awaiting merge): per-driver speed 0.80–1.25× limit (p5 0.84 / p95 1.13; highways faster, alleys slower), headway/accel/brake ranges, reaction delay at greens (8 % hesitant), casual amber runners, per-driver overtaking eagerness, indicator ≥ 0.5 s before lane changes; 0 civilian collisions, perf unchanged — numbers in `docs/integration/world.md` |
| 15 | City needs to be more dynamic: curves and elevation | helper `lead/city-v2`: Port Vesper-style procedural city, heightfield, curved/hill roads, terrain colliders | dispatched |
| 16 | Traffic lights not visible | helper `lead/playtest-signals`: signals run in the sim already; draw them in the browser | dispatched |
| 8 | Higher-power RWD cars: more steering assist / gyro so slides are controllable | helper `lead/playtest-drift`: slide-control assists in `wp_vehicle` (gyro yaw damper, rear-axle countersteer, slide throttle), keyboard/gamepad only | done on branch |
| 9 | "Give some relevant cars a little more steering angle to save the car" | helper `lead/playtest-drift`: lock opens while sliding at speed; more lock on 5 drift-capable RWD cars (shiro_turbo 39.8°, sperber_32 38.1°, rampart_50 37.9°, hakone_gt 35.9°, lucciola_350 33.1°) | done on branch |
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
| 7 | Camera zooms way too far out; need more angles | helper `lead/playtest-camera` | fixed on `lead/playtest-camera`: chase was a world-space spring trailing ≈12 m at chase speed → car-relative near chase 5.6 m back / 1.8 m up (≤1.2 m pull-back; 6.4 m measured at 115 km/h); 8 angles on C / 1–8 (near, far, hood, bumper, cockpit, side, TV, orbit), V hold look back, mouse/stick look-around, remembered per viewer; shots in `docs/media/cameras/` |
| 5 | Use real open-source sounds | J (audio): CC0 recorded packs | dispatched |
| 6 | Dispatcher and cops are way too talkative | helper `lead/playtest-radio`: `wp_police::radio` director (critical lines always; tactical/routine behind 8 s / 20 s global and 30 s per-unit gaps; duplicates merged; patrol chatter only when wanted or within 200 m; `data/police/radio.toml`) + HUD feed max 3 lines fading after 6 s. 5-min night city chase (`wp_police` scenario `radio_city_chase`, 4 seeds): **in pursuit 30.8 → 5.6 lines/min, not wanted 20.5 → 3.4/min**; calm patrol (`radio_calm_patrol`) 1.0 → 0.1/min. Goldens unchanged (radio not hashed) | fixed on `lead/playtest-radio` |
### Row 6 gate notes (`lead/playtest-radio`)
- fmt, `clippy --workspace --all-targets -D warnings`, `lint-determinism`, wasm32 build, ui-kit + testbed `tsc`, ui-kit vitest: pass.
- `cargo test --workspace`: everything passes except `simcli goldens_match`. Eight police-city goldens
  (city_chase, night_pursuit_escape, opponent ×4, race_city_sprint, traffic_soak) diverge at tick 24.
  This comes from the required merge of `origin/ws/police` (round 6 obstacle awareness): `origin/main` passes;
  the merge commit *before* any radio change fails with the same hashes as after it, so the radio change
  hashes the same. Nothing re-blessed here; E or the lead blesses with the police merge.
- `web-e2e`: everything passes except "city chase … speeding violations in the feed" (heat stays 0). It fails
  the same way on the merge commit without the radio change.
| 16 | Traffic lights not visible | helper `lead/playtest-signals`: `signal_heads()`/`signal_states()` wasm exports; mast-arm + far-side heads, stop lines, night glow (layer key 6); screenshots `docs/media/signals/` | fixed on `lead/playtest-signals` (not on the link yet) |
| 3 | Cars feel really slow | helper `lead/playtest-feel`: browser sim perf (sim fell behind real time at ~35–50 ms/frame in city chase) + car speed/feel | fixed on `lead/playtest-feel` (numbers below); not yet on the link |
| 4 | Transmission is sloppy | helper `lead/playtest-feel` (shift logic) | fixed on `lead/playtest-feel` (numbers below); not yet on the link |
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
| 14 | NPC cars need variance: lane changes, dynamic driving, different speeds | D (world): per-driver personality, MOBIL lane changes | dispatched (D round 6 follow-up) |
| 15 | City needs to be more dynamic: curves and elevation | helper `lead/city-v2`: Port Vesper-style procedural city, heightfield, curved/hill roads, terrain colliders | done on `lead/city-v2` (not merged): Port Vesper v2 is the city-chase default (selector keeps the grid) — downtown grid, organic districts, Hollins Drive switchbacks + Ridge Road crests (≈ 110 m hill), banked Loop 9 with an overpass and an underpass; terrain/road/building colliders; hill and loop scenarios at the limit pass (on road, never airborne); screenshots in `docs/media/city-v2/` |
| 17 | (lead helper, city v2) Port Vesper v2 needs a playtest on the published link: hills at speed, the Loop crest, the overpass/underpass, buildings now solid | owner → lead | to playtest (`?city=procedural_12x12` keeps the old grid) |

## 2026-10-19 — greybox demo end of run (lead helper `lead/demo-endrun`)
| # | Item | Owner | Status |
|---|---|---|---|
| 18 | Captured but nothing happens; no fine / impound / bounty; sirens not tied to the chase | lead (`wp_wasm::run`, testbed `src/ui/runEnd.tsx`) | done on `lead/demo-endrun`: capture / wreck holds the car, opens the result panel (fine, bail, impound fee, G's repair quote, cash before → after, ledger lines) + the bill; **Repair & continue** pays from the ledger and restores the car; escape → bounty toast; sirens/light bars follow unit assignment. Goldens unchanged. To playtest on the link |
| 19 | Engine sound from 2nd gear onwards doesn't work well | J (audio): reproduce with dumped rpm/gear/load/layer signals after the new gearbox; fix and add regression tests | dispatched |
