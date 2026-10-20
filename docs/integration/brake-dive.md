# Brake dive (owner playtest row 92) integration notes

- **Workstream:** lead helper, branch `lead/brake-dive` (from `origin/lead/round12`)
- **Row 92 (v0.12.12):** "headlights clip into the ground under braking": the front of the car
  (headlights / front bumper) visibly goes into the road under hard braking.
- **Touched:** `crates/wp_vehicle` (`suspension.rs`, `vehicle.rs` probe only, new `brake_dive.rs`,
  example, tests), `data/cars` (6 cars), `crates/wp_wasm` (`vehicle_info` wheel field),
  `apps/testbed-web` (wheel placement, e2e check), `crates/wp_sim/tests/brake_dive.rs`,
  `apps/simcli/scenarios/vehicle/brake_dive_100_{arcade,sim}.ron`, goldens (`[bless]`).
  Not touched: `steering.rs`, `slide.rs` (machine A).
- **Contract version built against:** 0.3.0 (no contract edits; one request below).

## 1. Cause: physics, not mesh placement

**Rendering checked first and is correct for the body.** `vehicle_view.ts` puts the car root at
the interpolated sim pose (`VehicleState.pos` = chassis reference point, `rot`), the panel meshes
(`render/panels.ts`) are in chassis space under that root, skinned to lattice nodes that only
move with contacts (an intact car's nodes sit at rest), the debug box spans 0.1·h…h, and the
city's roads are drawn at z = 0. The lowest panel vertex of every car is within 4 mm of its
lattice's lowest node (11–22 cm above the chassis origin, which sits on the road at rest), so the
drawn body is exactly where physics puts it. (One render bug found on the way, in the wheels,
not the body — §3.)

**Physics:** measured with `cargo run --release -p wp_vehicle --example brake_dive` (new
`wp_vehicle::brake_dive`: settle, cruise, full brake to a stop, wheel preset = a pedal step;
keyboard preset ramps it and gives the same numbers within 0.05°). Before (`origin/lead/round12`):

- **No anti-dive geometry.** Every tyre force acts at the contact patch and every bit of the
  braking load transfer (`m·a·h/L`) went through the springs. Peak nose-down pitch
  **1.7–3.8°** (real cars ≈ 1–2° at 1 g), front struts at **85–101 % of their travel**, the nose
  dropping 8–14 cm.
- **Low design clearance + that dive.** The lattice / panel body ahead of the front axle is
  11–22 cm above the road at rest and came down to **0.4 cm** (junkyard_beater), 0.7 cm
  (ironside_440), 1.7 cm (tansy_13). The chassis collider's front bottom corners (box bottom
  at 0.15 m over the full body length) **touched the road** on ironside_440, sentinel_bastion,
  drayman_cargo, furrow_24 (0.0 cm) — under the drawn body on the tall cars.
- **Bump stop only at the end of the travel** (linear-then-progressive from `travel_m`): nothing
  stiffened the last centimetres, furrow_24 squashed its bump stop 2 mm.
- **Soft/under-damped cars overshoot** at brake onset: the rear springs up past its steady
  height (beater rear strut 93 → 17 mm, settling at 45), so peak pitch was 1.3–1.5× the steady
  value on the five softest cars.
- Keyboard players also hold the brake key through the stop (auto-reverse then drives
  backwards on the same key, which loads the front again); measured separately, never worse than
  the stop itself.

## 2. Fix

`wp_vehicle::suspension` (identical in Arcade and Simulation; the crate never reads the mode):
- **Anti-dive / anti-lift** (`ANTI_DIVE_FRONT = 0.55`, `ANTI_LIFT_REAR = 0.4`,
  `anti_pitch_factor`): the links carry that share of the braking load transfer. Per wheel, a
  jacking force `factor × fx_brake` (factor = ∓anti·h/L) acts on the chassis at the wheel *and*
  on the tyre's normal load, so axle loads still shift by the full `m·a·h/L` (grip and braking
  balance unchanged); only the springs, and with them pitch, see less. `fx_brake` is the tyre's
  longitudinal force clamped to what the brake torque (the pedal's service torque, so ABS
  release cycles don't flicker it) can react: drive / engine braking and a kerb strike's push on
  a free-rolling wheel jack nothing (rows 54/55: no launch from kerbs), anti-squat is not
  modelled. Faded in between 1 and 2 m/s contact speed: at a standstill the held tyre's stiff
  grip otherwise coupled heave into the links (kestrel heave ζ fell to 0.06 before the fade).
- **Bump stops: no code change.** They were already progressive with hysteresis (round 12b).
  A 2.5 cm "bump rubber" engaging before the end of the travel was built and **removed**: with
  anti-dive no front strut gets within 17 % of its travel in a 1 g stop (it changed no
  brake-dive number), while it stiffened kerb strikes (kestrel_ts 15 cm kerb at 60: body rise
  4.7 → 6.1 cm, at the row-54 test limit) and moved lucciola_350's 2-way vs locked lift-off
  margin. Instead the geometry is checked: with **both front struts on their bump stops** (rear
  at rest) the body ahead of the front axle is still 1.2 cm (lucciola_350) … 11 cm (hauler_1500)
  above the road (`brake_dive::clearance_on_front_bump_stops`, test > 1 cm).

`data/cars` (H's folder; the five cars still above ≈ 2.5° with the geometry alone, all soft
"character" cars): ironside_440 and furrow_24 dampers ×1.35; tansy_13 dampers ×1.35 (rear
rebound ×1.8) and springs ×1.1; junkyard_beater front dampers ×1.3, rear bump ×1.6 / rebound
×2.0 (still the softest-damped car); sentinel_bastion dampers ×1.3, springs ×1.12.
**Travel:** lindwurm_4t front `travel_m` 0.15 → 0.135 (bump stop 1.5 cm earlier: body on the bump stops 1.0 → 3.0 cm off the road;
7.4 cm bump room left at rest). The same on lucciola_350 (1.2 cm) made it airborne on the 15 cm
kerb, so it keeps its data. Ride frequency / damping / bump-room bands in `tests/ride.rs` hold
(bump ζ ≤ 0.45, rebound > 1.6 × bump, > 5 cm bump room).
**H:** please carry these into the `*.spec.md` files and `VALIDATION.md`.

No ride-height change was needed: with the fix the minimum front body clearance in a stop is
5.7 cm and the collider corners stay ≥ 4.4 cm off the road.

## 3. Rendering fix (wheels)

The testbed drew each wheel centre at `position_m.z + compression − travel/2`, but physics puts
the hub at `position_m` at the **static** compression, so tyres floated or sank 1–3 cm
(bastion +2.8 cm). `vehicle_info` / `car_model_info` wheels now carry `static_compression_m`
(as `wp_vehicle` computes it) and `wasm/layout.ts::wheelCentreZ` is used by `vehicle_view.ts`
and the skid-mark contact in `render/fx/index.ts` (falls back to travel/2 for stand-in props).
Measured in the browser: drawn tyre bottoms within −0.1…0.5 cm of the road through the stop.

## 4. Before → after (every roster car, flat asphalt, wheel preset, 100 → 0 and 60 → 0)

Peak / steady pitch: nose-down relative to the car at rest (steady = mean between 80 % and 20 %
of the start speed). Front strut: largest compression / travel. Front body: lowest lattice node
ahead of the front axle above the road (at rest in brackets). Collider: the chassis box's front
bottom corners.

| car | km/h | decel g | peak pitch ° | steady pitch ° | front strut / travel | front body clearance cm (at rest) | collider corner cm |
|---|---|---|---|---|---|---|---|
| vireo_xs | 100 | 1.00 → **1.01** | 2.52 → **1.92** | 2.08 → **1.64** | 0.85 → **0.74** | 4.9 → **8.2** (14.0) | 6.2 → **9.4** |
| vireo_xs | 60 | 0.99 → **0.99** | 2.41 → **1.82** | 2.13 → **1.66** | 0.84 → **0.73** | 5.3 → **8.2** (14.0) | 6.6 → **9.4** |
| kestrel_ts | 100 | 1.00 → **1.01** | 2.84 → **2.17** | 2.27 → **1.79** | 0.94 → **0.79** | 3.1 → **7.0** (14.0) | 4.0 → **7.9** |
| kestrel_ts | 60 | 0.98 → **0.98** | 2.73 → **2.08** | 2.35 → **1.82** | 0.93 → **0.79** | 3.5 → **7.2** (14.0) | 4.4 → **8.1** |
| shiro_turbo | 100 | 1.02 → **1.02** | 1.95 → **1.44** | 1.55 → **1.19** | 0.73 → **0.62** | 5.4 → **8.0** (13.0) | 7.2 → **9.9** |
| shiro_turbo | 60 | 0.98 → **0.99** | 1.85 → **1.40** | 1.52 → **1.17** | 0.72 → **0.62** | 5.8 → **8.0** (13.0) | 7.6 → **9.9** |
| hakone_gt | 100 | 0.98 → **0.99** | 1.98 → **1.47** | 1.52 → **1.15** | 0.73 → **0.62** | 4.8 → **7.5** (13.0) | 6.7 → **9.5** |
| hakone_gt | 60 | 0.95 → **0.95** | 1.88 → **1.39** | 1.48 → **1.12** | 0.72 → **0.62** | 5.2 → **7.8** (13.0) | 7.1 → **9.7** |
| rampart_50 | 100 | 0.98 → **0.99** | 2.39 → **1.78** | 1.89 → **1.44** | 0.90 → **0.77** | 2.9 → **6.3** (13.0) | 4.5 → **8.0** |
| rampart_50 | 60 | 0.94 → **0.94** | 2.27 → **1.69** | 1.86 → **1.40** | 0.88 → **0.76** | 3.4 → **6.5** (13.0) | 5.1 → **8.3** |
| ironside_440 | 100 | 0.74 → **0.75** | 3.28 → **2.33** | 2.53 → **1.98** | 0.95 → **0.77** | 0.7 → **6.1** (15.0) | 0.0 → **5.6** |
| ironside_440 | 60 | 0.72 → **0.73** | 3.26 → **2.39** | 2.55 → **2.00** | 0.95 → **0.79** | 0.7 → **5.7** (15.0) | 0.0 → **5.2** |
| sperber_32 | 100 | 1.02 → **1.03** | 1.68 → **1.25** | 1.37 → **1.03** | 0.72 → **0.62** | 5.8 → **8.2** (13.0) | 7.7 → **10.1** |
| sperber_32 | 60 | 0.99 → **0.99** | 1.62 → **1.20** | 1.35 → **1.02** | 0.71 → **0.61** | 6.1 → **8.3** (13.0) | 8.0 → **10.2** |
| lindwurm_4t | 100 | 1.04 → **1.04** | 2.13 → **1.61** | 1.79 → **1.39** | 0.67 → **0.62** | 5.0 → **7.9** (13.0) | 7.3 → **10.0** |
| lindwurm_4t | 60 | 1.02 → **1.03** | 2.14 → **1.61** | 1.78 → **1.39** | 0.67 → **0.62** | 4.9 → **7.8** (13.0) | 7.2 → **10.1** |
| lucciola_350 | 100 | 1.06 → **1.06** | 1.73 → **1.29** | 1.35 → **1.02** | 0.73 → **0.62** | 3.4 → **5.7** (11.0) | 7.3 → **9.5** |
| lucciola_350 | 60 | 1.02 → **1.02** | 1.65 → **1.23** | 1.32 → **0.99** | 0.71 → **0.61** | 3.8 → **5.9** (11.0) | 7.6 → **9.8** |
| hauler_1500 | 100 | 0.86 → **0.86** | 2.35 → **1.73** | 1.89 → **1.43** | 0.96 → **0.83** | 9.8 → **14.2** (22.0) | 2.2 → **6.8** |
| hauler_1500 | 60 | 0.83 → **0.84** | 2.29 → **1.69** | 1.90 → **1.43** | 0.95 → **0.83** | 10.0 → **14.3** (22.0) | 2.4 → **6.8** |
| sentinel_cruiser | 100 | 0.95 → **0.96** | 2.05 → **1.53** | 1.71 → **1.30** | 0.92 → **0.80** | 4.6 → **7.7** (14.0) | 5.5 → **8.7** |
| sentinel_cruiser | 60 | 0.92 → **0.92** | 1.95 → **1.45** | 1.68 → **1.28** | 0.91 → **0.80** | 5.0 → **7.9** (14.0) | 5.9 → **8.8** |
| sentinel_wolfhound | 100 | 0.96 → **0.95** | 2.53 → **1.90** | 1.98 → **1.48** | 0.91 → **0.77** | 4.0 → **7.7** (15.0) | 3.6 → **7.4** |
| sentinel_wolfhound | 60 | 0.93 → **0.91** | 2.40 → **1.80** | 1.96 → **1.47** | 0.89 → **0.76** | 4.5 → **8.0** (15.0) | 4.1 → **7.8** |
| sentinel_bastion | 100 | 0.88 → **0.89** | 3.26 → **2.49** | 3.15 → **2.16** | 0.99 → **0.79** | 7.9 → **12.1** (22.0) | 0.0 → **4.4** |
| sentinel_bastion | 60 | 0.85 → **0.86** | 3.18 → **2.38** | 3.04 → **2.13** | 0.99 → **0.79** | 7.9 → **12.2** (22.0) | 0.0 → **4.6** |
| junkyard_beater | 100 | 0.84 → **0.85** | 3.80 → **2.45** | 2.52 → **2.00** | 0.97 → **0.76** | 0.4 → **6.1** (14.0) | 1.4 → **7.1** |
| junkyard_beater | 60 | 0.79 → **0.80** | 3.41 → **2.18** | 2.43 → **1.89** | 0.93 → **0.75** | 1.7 → **6.7** (14.0) | 2.7 → **7.7** |
| tansy_13 | 100 | 0.97 → **0.98** | 3.66 → **2.36** | 2.97 → **2.13** | 1.00 → **0.70** | 1.7 → **6.9** (14.0) | 3.1 → **8.1** |
| tansy_13 | 60 | 0.96 → **0.97** | 3.49 → **2.36** | 3.06 → **2.13** | 0.98 → **0.70** | 2.3 → **6.9** (14.0) | 3.6 → **8.1** |
| drayman_cargo | 100 | 0.89 → **0.90** | 2.76 → **2.02** | 2.18 → **1.63** | 0.99 → **0.81** | 6.1 → **11.2** (20.0) | -0.0 → **5.4** |
| drayman_cargo | 60 | 0.86 → **0.86** | 2.67 → **1.95** | 2.20 → **1.63** | 0.97 → **0.80** | 6.5 → **11.4** (20.0) | 0.4 → **5.6** |
| furrow_24 | 100 | 0.86 → **0.87** | 3.50 → **2.40** | 2.58 → **1.99** | 1.01 → **0.81** | 5.9 → **11.8** (20.0) | 0.0 → **6.2** |
| furrow_24 | 60 | 0.80 → **0.81** | 3.35 → **2.26** | 2.49 → **1.89** | 1.00 → **0.81** | 6.3 → **11.9** (20.0) | 0.5 → **6.3** |

Real-car target ≈ 1–2° at 1 g: sports/sedans now 1.2–2.2°, soft muscle / SUV / beater / vans
2.0–2.5°. No bump stop is reached any more (front struts ≤ 83 % of their travel; was 85–101 %, 2 mm
of bump-stop squash on furrow_24).
Braking deceleration unchanged (±0.01 g).

**Keyboard preset** (pedal ramp) gives the same peaks within 0.05°; **holding the brake into
reverse** (keyboard) never exceeds the stop itself.

**Through `wp_sim`, both modes** (`crates/wp_sim/tests/brake_dive.rs`, all 17 cars × Arcade /
Simulation, 100 → 0): peak ≤ 2.5°, front lattice body > 4 cm, zero chassis contacts with the
road. `vehicle/brake_dive_100_arcade.ron` and `_sim.ron` give the **same state hash**
(921b9c1133fca428) — physics identical in both modes.

**Browser** (`npm run e2e:brake-dive`, headless Chromium, both scenarios, drawn panel-mesh
vertices every frame): lowest drawn front body vertex 7.2 cm (ironside_440), 7.4 cm
(junkyard_beater), 13.2 cm (sentinel_bastion) above the road; drawn nose dive 2.4–2.6° (from the
first frame, which is still slightly nose-up from cruising); tyres on the road ±0.5 cm.

## 5. Handling balance and ride (rows 28, 54/55)

Handling (`--example handling`, wheel preset, 80 km/h; before = `origin/lead/round12`). Anti-dive
acts only on brake-reacted force, so cornering without the brake is changed only by the six
cars' damper / spring / travel data. Last column: body ahead of the front axle above the road
with both front struts on their bump stops (geometry, after).

| car | US gradient deg/g | ramp max g | full-input g | body on front bump stops cm |
|---|---|---|---|---|
| vireo_xs | 0.42 → **0.42** | 0.87 → **0.87** | 0.75 → **0.75** | 4.6 |
| kestrel_ts | 0.31 → **0.31** | 0.94 → **0.94** | 0.80 → **0.80** | 5.3 |
| shiro_turbo | 0.34 → **0.34** | 0.96 → **0.96** | 0.66 → **0.66** | 3.1 |
| hakone_gt | 0.15 → **0.15** | 0.94 → **0.94** | 0.66 → **0.66** | 2.2 |
| rampart_50 | 0.22 → **0.22** | 0.90 → **0.90** | 0.64 → **0.64** | 3.5 |
| ironside_440 | 1.40 → **1.39** | 0.64 → **0.64** | 0.37 → **0.37** | 2.9 |
| sperber_32 | 0.18 → **0.18** | 0.95 → **0.95** | 0.65 → **0.65** | 2.7 |
| lindwurm_4t | 0.32 → **0.32** | 0.95 → **0.95** | 0.75 → **0.75** | 3.0 |
| lucciola_350 | 0.07 → **0.07** | 1.00 → **1.00** | 0.71 → **0.71** | 1.2 |
| hauler_1500 | 1.44 → **1.44** | 0.66 → **0.66** | 0.52 → **0.52** | 11.1 |
| sentinel_cruiser | 0.29 → **0.29** | 0.86 → **0.86** | 0.64 → **0.64** | 5.2 |
| sentinel_wolfhound | 0.26 → **0.26** | 0.88 → **0.88** | 0.72 → **0.72** | 5.0 |
| sentinel_bastion | 0.41 → **0.40** | 0.75 → **0.76** | 0.58 → **0.58** | 8.7 |
| junkyard_beater | 1.60 → **1.59** | 0.71 → **0.71** | 0.45 → **0.45** | 4.0 |
| tansy_13 | 0.50 → **0.48** | 0.82 → **0.81** | 0.70 → **0.70** | 3.4 |
| drayman_cargo | 0.69 → **0.69** | 0.68 → **0.68** | 0.52 → **0.52** | 8.6 |
| furrow_24 | 0.47 → **0.49** | 0.75 → **0.75** | 0.58 → **0.58** | 9.6 |

`tests/handling_balance.rs` **passes** (on `origin/lead/round12` it fails:
junkyard_beater 1.60x deg/g > 1.6 in `front_drive_cars_understeer_mildly`; now 1.59).

Ride (`--example ride`, the four reference cars: kerbs 10/15 cm at 30/60, crests, slides into
kerbs, side impacts, heave/pitch kicks): **identical** to `origin/lead/round12` (no brake in
those runs, so no jacking; their data is unchanged). `tests/ride.rs` passes for the roster.

## 6. Tests and scenarios
- `crates/wp_vehicle/tests/brake_dive.rs`: every roster car, wheel + keyboard, 100 and 60 km/h:
  peak ≤ 2.5°, steady ≤ 2.2°, still dives > 0.8°, front body > 4 cm, collider > 3 cm, never past
  the end of the travel, decel > 0.6 g; holding the brake into reverse ≤ 2.5°.
- `crates/wp_sim/tests/brake_dive.rs`: the two-mode game-loop check above (fails on the old
  physics: vireo_xs already 2.50° with anti-dive off).
- `apps/simcli/scenarios/vehicle/brake_dive_100_{arcade,sim}.ron`: three soft cars, stop
  distance 38–65 m, no crush (pitch is in the wp_sim test: simcli has no pitch metric).
- `apps/testbed-web/e2e/brake_dive.mjs` (`npm run e2e:brake-dive`; not added to `npm run e2e`
  to keep that run's length; lead's call).
- Same file: `the_bump_stops_hold_the_nose_off_the_road` (geometry, every car > 1 cm).

## Contract change requests
| # | Change | Why | Who is affected | Status |
|---|---|---|---|---|
| 1 | Optional `SuspensionDef::anti_dive_frac: Option<f32>` (front corners; default 0.55) and `anti_lift_frac: Option<f32>` (rear corners; default 0.4) | per-car geometry (trucks / old cars have less anti-dive, sports cars more); today they are `wp_vehicle` constants for every car | lead (contracts, schemas), H (car data), A (wp_vehicle reads them) | open |

## Changes outside wp_vehicle
- `crates/wp_wasm/src/lib.rs`: `WheelInfo::static_compression_m` (lead crate; additive JSON field).
- `apps/testbed-web`: `wasm/layout.ts` (`wheelCentreZ`), `wasm/sim.ts` (type), `vehicle_view.ts`,
  `render/fx/index.ts`, `e2e/brake_dive.mjs`, `package.json` (`e2e:brake-dive`).
- `data/cars/*.json`: 5 cars (above). Goldens + pinned hashes re-recorded in a `[bless]` commit.

## Known gaps / notes for others
- Anti-squat under acceleration is not modelled (drive torque jacks nothing).
- The chassis collider is still a box with its bottom 0.15 m over the full body length (the
  tall cars' drawn body is 20–22 cm up): a lower-than-drawn collider can scrape on crests; now
  ≥ 4.4 cm away under braking. A per-car floor from the lattice would match the drawn body.
- Lattice bottoms are 11–15 cm on the cars (real bumpers ~15–20 cm); not changed (B/H's data).
  lucciola_350 has only 1.2 cm left with its front struts on the bump stops (a hard kerb or
  landing can still squash the bump stop that far); a higher nose in its lattice/mesh would fix
  it without touching its ride.

## Requests
- **F (net):** `wp_net/tests/vesper_e2e.rs::four_clients_three_minutes_in_port_vesper` fails
  here (passes on `origin/lead/round12`): "p1 never saw racer 0's pursuit end". Traced: racer 0
  is now captured at step 2854, inside the test's scripted disconnect of client p1 (steps
  2700–2880); p0/p2/p3 get the `PursuitEnded` at 2854–2856. Game events queued while a client is
  offline are never replayed on reconnect (`Server::send` returns when `conn` is `None`), so p1
  cannot see it. The vehicle change only moved the capture earlier (the racer is boxed in
  before its scripted give-up at 3185). Suggest replaying the events a reconnecting player
  missed (at least `PursuitEnded` / `WantedLevelChanged` for their own racer and pursuits they
  took part in), or keeping the capture out of the drop window in the test.
- **H (cars):** carry the six cars' data changes (§2) into the `*.spec.md` files and
  `VALIDATION.md`.
- **Lead (contracts):** request #1 above.
- **Lead:** decide whether `e2e/brake_dive.mjs` joins `npm run e2e`.

## Checks (`lead/brake-dive`, 2026-10-20; CARGO_INCREMENTAL=0, no debug info)
- `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`: clean.
- `cargo build --target wasm32-unknown-unknown -p wp_wasm`, `npm run wasm`, `tsc --noEmit`: ok.
- `npm run e2e:brake-dive` (headless Chromium): all checks pass (numbers in §4).
- `simcli run apps/simcli/scenarios` (84): all pass except `police_tunnel_escape_founders_cut`
  (racer captured at 135 s instead of escaping) — **fails identically on `origin/lead/round12`**
  (captured at 133.8 s).
- `simcli goldens` after the `[bless]`: all match except the four scenarios that have **no golden
  on `origin/lead/round12` either** (`city_tunnel_{founders_cut,hollins}_drive_through`,
  `police_tunnel_escape_founders_cut{,_control}`; not added here).
- `cargo test --workspace --no-fail-fast` (debug): 147 test binaries ok; failures:
  - `wp_audio engine_shift::rev_matched_downshifts_blip` (1.1 dB < 2): **same on the base**
    (known since round 12b, J).
  - `wp_race race_picker`: `loop9_circuit` (**fails on the base too**), `grid_highway_sprint`,
    `hollins_hill_sprint` (pass on the base; two AI racers DNF at gate 3 queued behind
    traffic, the pattern round 12b recorded for `grid_highway_sprint`), and `wp_wasm
    race_tests::short_sprint_…` (the same grid sprint; round 12b: fails on round12 too). On the
    base `lantern_checkpoint_draws_the_police` fails instead. These are 1–5 minute AI-in-traffic
    races: any vehicle change reshuffles them (L / N).
  - `wp_net vesper_e2e::four_clients_three_minutes_in_port_vesper`: see Requests (F).
- Determinism: `vehicle/brake_dive_100_arcade` and `_sim` produce the same hash in both modes;
  the goldens re-run after the bless match (in-process determinism).
