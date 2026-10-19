# Playtest log (owner feedback)

## 2026-10-19 — first city chase on the published link
| # | Feedback | Owner of the fix | Status |
|---|---|---|---|
| 1 | No city buildings on the published link | lead (artifact can't serve .glb → base64 fallback in `scene.ts`) | fixed, link v5 |
| 2 | No visible car damage | lead: lattice view on by default; real denting panels = R (prompt 21) | interim fix on link |
| 3 | Cars feel really slow | helper `lead/playtest-feel`: browser sim perf (sim fell behind real time at ~35–50 ms/frame in city chase) + car speed/feel | dispatched |
| 4 | Transmission is sloppy | helper `lead/playtest-feel` (shift logic) | dispatched |
| 5 | Use real open-source sounds | J (audio): CC0 recorded packs | dispatched |
| 6 | Dispatcher and cops are way too talkative | helper `lead/playtest-radio` | dispatched |
| 7 | Camera zooms way too far out; need more angles | helper `lead/playtest-camera` | dispatched |
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
