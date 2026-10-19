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
