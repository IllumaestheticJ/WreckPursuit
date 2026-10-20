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
keyboard preset ramps it and gives the same numbers within 0.06°). Before (first measured on
`origin/lead/round12` @ ea90bf7; after round12's beater rework the beater was worse — 4.1°, front
body 1.0 cm into the road, §4):

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
- **Anti-dive / anti-lift** (`ANTI_DIVE_FRONT = 0.7`, `ANTI_LIFT_REAR = 0.6`,
  `anti_pitch_factor`; first 0.55 / 0.4, raised after the round12 merge so the soft cars need
  damping, not stiffer springs; 0.75 rear was tried and flipped the chaotic
  `police_tunnel_escape_founders_cut` chase, so 0.6): the links carry that share of the braking load transfer. Per wheel, a
  jacking force `factor × fx_brake` (factor = ∓anti·h/L) acts on the chassis at the wheel *and*
  on the tyre's normal load, so axle loads still shift by the full `m·a·h/L` (grip and braking
  balance unchanged); only the springs, and with them pitch, see less. `fx_brake` is the tyre's
  longitudinal force, **rearward only**, clamped to what the brake torque (the pedal's service
  torque, so ABS release cycles don't flicker it) can hold: a TCS / ESC brake on a driven wheel
  that still pushes forward jacks nothing, so full-throttle launches are bit-identical to
  `lead/round12`; drive / engine braking and a kerb strike's push on a free-rolling wheel jack
  nothing (rows 54/55: no launch from kerbs), anti-squat is not
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

`data/cars` (H's folder) — **more damping, not stiffer** (lead, after the owner's "far too
switchy" and Drift Lab W's damping-up defaults). Multipliers on `lead/round12`'s data:

| car | front bump / rebound | rear bump / rebound | other |
|---|---|---|---|
| ironside_440 | ×1.35 / ×1.35 | ×1.35 / ×1.35 | |
| furrow_24 | ×1.35 / ×1.35 | ×1.35 / ×1.35 | |
| tansy_13 | ×1.35 / ×1.35 | ×1.35 / ×1.8 | |
| junkyard_beater (round12's reworked beater) | ×2.0 / ×2.0 | ×2.0 / ×2.8 | |
| sentinel_bastion | ×1.3 / ×1.8 | ×1.3 / ×2.0 | |
| lindwurm_4t | | | front `travel_m` 0.15 → 0.135 |

No spring is stiffer (the first pass had bastion ×1.12 / tansy ×1.1; reverted). Bump ζ stays
inside the ride-test band (≤ 0.45), rebound > 1.6 × bump. lindwurm_4t: bump stop 1.5 cm
earlier, body on the bump stops 1.0 → 3.0 cm off the road, 7.4 cm bump room left at rest (the
same on lucciola_350 made it airborne on the 15 cm kerb, so it keeps its data).
**H:** please carry these into the `*.spec.md` files and `VALIDATION.md`.

No ride-height change was needed: with the fix the minimum front body clearance in a stop is
5.4 cm and the collider corners stay ≥ 3.7 cm off the road.

## 3. Rendering fix (wheels)

The testbed drew each wheel centre at `position_m.z + compression − travel/2`, but physics puts
the hub at `position_m` at the **static** compression, so tyres floated or sank 1–3 cm
(bastion +2.8 cm). `vehicle_info` / `car_model_info` wheels now carry `static_compression_m`
(as `wp_vehicle` computes it) and `wasm/layout.ts::wheelCentreZ` is used by `vehicle_view.ts`
and the skid-mark contact in `render/fx/index.ts` (falls back to travel/2 for stand-in props).
Measured in the browser: drawn tyre bottoms within −0.1…0.5 cm of the road through the stop.

## 4. Before (`origin/lead/round12` @ 848465d3) → after (every roster car, flat asphalt, wheel preset, 100 → 0 and 60 → 0)

Peak / steady pitch: nose-down relative to the car at rest (steady = mean between 80 % and 20 %
of the start speed). Front strut: largest compression / travel. Front body: lowest lattice node
ahead of the front axle above the road (at rest in brackets). Collider: the chassis box's front
bottom corners.

| car | km/h | decel g | peak pitch ° | steady pitch ° | front strut / travel | front body clearance cm (at rest) | collider corner cm |
|---|---|---|---|---|---|---|---|
| vireo_xs | 100 | 0.99 → **0.98** | 2.52 → **1.77** | 2.02 → **1.48** | 0.85 → **0.74** | 4.8 → **8.1** (14.0) | 6.2 → **9.4** |
| vireo_xs | 60 | 0.95 → **0.97** | 2.25 → **1.77** | 2.04 → **1.49** | 0.82 → **0.74** | 5.8 → **8.0** (14.0) | 7.1 → **9.2** |
| kestrel_ts | 100 | 0.98 → **0.96** | 2.84 → **1.99** | 2.22 → **1.61** | 0.94 → **0.80** | 3.1 → **7.3** (14.0) | 4.0 → **8.2** |
| kestrel_ts | 60 | 0.94 → **0.98** | 2.55 → **2.03** | 2.22 → **1.69** | 0.91 → **0.80** | 4.2 → **7.4** (14.0) | 5.1 → **8.3** |
| shiro_turbo | 100 | 1.02 → **1.01** | 1.95 → **1.36** | 1.55 → **1.06** | 0.73 → **0.62** | 5.4 → **8.0** (13.0) | 7.2 → **9.9** |
| shiro_turbo | 60 | 0.98 → **0.98** | 1.85 → **1.37** | 1.52 → **1.04** | 0.72 → **0.62** | 5.8 → **8.0** (13.0) | 7.6 → **9.8** |
| hakone_gt | 100 | 0.98 → **0.98** | 1.98 → **1.36** | 1.52 → **1.03** | 0.73 → **0.62** | 4.8 → **7.7** (13.0) | 6.7 → **9.6** |
| hakone_gt | 60 | 0.94 → **0.95** | 1.88 → **1.37** | 1.47 → **1.01** | 0.72 → **0.62** | 5.2 → **7.7** (13.0) | 7.1 → **9.6** |
| rampart_50 | 100 | 0.98 → **0.99** | 2.39 → **1.58** | 1.87 → **1.30** | 0.90 → **0.76** | 2.9 → **6.8** (13.0) | 4.5 → **8.6** |
| rampart_50 | 60 | 0.93 → **0.96** | 2.27 → **1.56** | 1.81 → **1.29** | 0.88 → **0.76** | 3.3 → **6.9** (13.0) | 5.0 → **8.6** |
| ironside_440 | 100 | 0.74 → **0.75** | 3.28 → **2.10** | 2.53 → **1.75** | 0.95 → **0.76** | 0.7 → **7.0** (15.0) | 0.0 → **6.6** |
| ironside_440 | 60 | 0.72 → **0.72** | 3.26 → **2.02** | 2.55 → **1.74** | 0.95 → **0.75** | 0.7 → **7.4** (15.0) | 0.0 → **7.0** |
| sperber_32 | 100 | 1.02 → **1.01** | 1.68 → **1.20** | 1.36 → **0.92** | 0.72 → **0.62** | 5.8 → **8.2** (13.0) | 7.7 → **10.1** |
| sperber_32 | 60 | 0.98 → **0.98** | 1.61 → **1.20** | 1.34 → **0.90** | 0.71 → **0.62** | 6.1 → **8.2** (13.0) | 8.0 → **10.1** |
| lindwurm_4t | 100 | 1.03 → **1.01** | 2.11 → **1.50** | 1.76 → **1.22** | 0.66 → **0.63** | 5.1 → **7.8** (13.0) | 7.3 → **10.0** |
| lindwurm_4t | 60 | 1.00 → **1.00** | 2.08 → **1.49** | 1.76 → **1.21** | 0.66 → **0.63** | 5.1 → **7.8** (13.0) | 7.4 → **9.9** |
| lucciola_350 | 100 | 1.04 → **1.04** | 1.73 → **1.23** | 1.33 → **0.89** | 0.73 → **0.61** | 3.4 → **5.9** (11.0) | 7.2 → **9.8** |
| lucciola_350 | 60 | 1.00 → **1.00** | 1.66 → **1.24** | 1.30 → **0.87** | 0.71 → **0.61** | 3.8 → **5.9** (11.0) | 7.6 → **9.8** |
| hauler_1500 | 100 | 0.86 → **0.86** | 2.35 → **1.59** | 1.89 → **1.27** | 0.96 → **0.83** | 9.8 → **14.4** (22.0) | 2.2 → **7.0** |
| hauler_1500 | 60 | 0.83 → **0.84** | 2.29 → **1.58** | 1.90 → **1.27** | 0.95 → **0.83** | 10.0 → **14.5** (22.0) | 2.4 → **7.0** |
| sentinel_cruiser | 100 | 0.95 → **0.95** | 2.05 → **1.46** | 1.71 → **1.15** | 0.92 → **0.80** | 4.6 → **7.8** (14.0) | 5.5 → **8.8** |
| sentinel_cruiser | 60 | 0.92 → **0.93** | 1.95 → **1.43** | 1.68 → **1.15** | 0.91 → **0.80** | 5.0 → **7.9** (14.0) | 5.9 → **8.9** |
| sentinel_wolfhound | 100 | 0.96 → **0.95** | 2.53 → **1.72** | 1.98 → **1.32** | 0.91 → **0.76** | 4.0 → **8.5** (15.0) | 3.6 → **8.2** |
| sentinel_wolfhound | 60 | 0.93 → **0.93** | 2.40 → **1.70** | 1.96 → **1.33** | 0.89 → **0.76** | 4.5 → **8.5** (15.0) | 4.1 → **8.2** |
| sentinel_bastion | 100 | 0.88 → **0.89** | 3.26 → **2.52** | 3.15 → **2.12** | 0.99 → **0.89** | 7.9 → **11.4** (22.0) | 0.0 → **3.7** |
| sentinel_bastion | 60 | 0.85 → **0.86** | 3.18 → **2.50** | 3.04 → **2.04** | 0.99 → **0.89** | 7.9 → **11.5** (22.0) | 0.0 → **3.8** |
| junkyard_beater | 100 | 0.81 → **0.83** | 4.13 → **2.49** | 3.16 → **2.36** | 1.03 → **0.82** | -1.0 → **5.4** (14.0) | 0.0 → **6.4** |
| junkyard_beater | 60 | 0.75 → **0.81** | 3.99 → **2.48** | 2.98 → **2.26** | 1.03 → **0.82** | -0.7 → **5.5** (14.0) | 0.3 → **6.5** |
| tansy_13 | 100 | 0.93 → **0.94** | 3.68 → **2.41** | 2.83 → **2.09** | 1.00 → **0.80** | 1.7 → **6.6** (14.0) | 3.0 → **7.9** |
| tansy_13 | 60 | 0.92 → **0.95** | 3.56 → **2.41** | 2.86 → **2.14** | 0.99 → **0.80** | 2.0 → **6.6** (14.0) | 3.4 → **7.8** |
| drayman_cargo | 100 | 0.89 → **0.89** | 2.76 → **1.80** | 2.18 → **1.45** | 0.99 → **0.80** | 6.1 → **11.8** (20.0) | -0.0 → **6.1** |
| drayman_cargo | 60 | 0.86 → **0.86** | 2.67 → **1.80** | 2.20 → **1.43** | 0.97 → **0.80** | 6.5 → **11.6** (20.0) | 0.4 → **5.9** |
| furrow_24 | 100 | 0.89 → **0.91** | 3.50 → **2.17** | 2.60 → **1.81** | 1.01 → **0.81** | 5.9 → **11.9** (20.0) | 0.0 → **6.4** |
| furrow_24 | 60 | 0.83 → **0.85** | 3.35 → **2.17** | 2.52 → **1.73** | 1.00 → **0.81** | 6.3 → **11.9** (20.0) | 0.5 → **6.3** |

Real-car target ≈ 1–2° at 1 g: sports / sedans / police now 1.2–2.0°, soft muscle / SUV /
beater / economy 2.0–2.52°. Before, round12's softer beater reached 4.1° and put its front
body **1.0 cm into the road**. No bump stop is reached any more (front struts ≤ 89 % of their travel; was up to 103 %).
Braking deceleration unchanged (±0.01 g).

**Keyboard preset** (pedal ramp) gives the same peaks within 0.06°; the **owner default** (keyboard, TCS / ESC /
automatic gears off — manual gears, so engine braking adds to the stop) peaks at 2.51° (bastion,
beater); **holding the brake into
reverse** (keyboard) never exceeds the stop itself.

**Through `wp_sim`, both modes** (`crates/wp_sim/tests/brake_dive.rs`, all 17 cars × Arcade /
Simulation, 100 → 0): peak ≤ 2.55°, front lattice body > 4 cm, zero chassis contacts with the
road. `vehicle/brake_dive_100_arcade.ron` and `_sim.ron` give the **same state hash** — physics identical in both modes.

**Browser** (`npm run e2e:brake-dive`, headless Chromium, both scenarios, drawn panel-mesh
vertices every frame): lowest drawn front body vertex 7.2 cm (ironside_440), 7.4 cm
(junkyard_beater), 13.2 cm (sentinel_bastion) above the road; drawn nose dive 2.4–2.6° (from the
first frame, which is still slightly nose-up from cruising); tyres on the road ±0.5 cm.

## 5. Handling, ride, body damping (rows 28, 54/55, Drift Lab W)

Handling (`--example handling`, wheel preset, 80 km/h; before = `origin/lead/round12`). Anti-dive
acts only on rearward brake-held tyre force, so cornering without the brake changes only through
the six cars' data. Last column: body ahead of the front axle above the road with both front
struts on their bump stops (geometry, after).

| car | US gradient deg/g | ramp max g | full-input g | body on front bump stops cm |
|---|---|---|---|---|
| vireo_xs | 0.42 → **0.42** | 0.87 → **0.87** | 0.75 → **0.75** | 4.6 |
| kestrel_ts | 0.40 → **0.40** | 0.94 → **0.94** | 0.80 → **0.80** | 5.3 |
| shiro_turbo | 0.34 → **0.34** | 0.96 → **0.96** | 0.66 → **0.66** | 3.1 |
| hakone_gt | 0.12 → **0.12** | 0.94 → **0.94** | 0.66 → **0.66** | 2.2 |
| rampart_50 | 0.23 → **0.23** | 0.90 → **0.90** | 0.59 → **0.59** | 3.5 |
| ironside_440 | 1.40 → **1.39** | 0.64 → **0.64** | 0.37 → **0.37** | 2.9 |
| sperber_32 | 0.18 → **0.18** | 0.96 → **0.96** | 0.65 → **0.65** | 2.7 |
| lindwurm_4t | 0.31 → **0.31** | 0.95 → **0.95** | 0.75 → **0.75** | 3.0 |
| lucciola_350 | 0.07 → **0.07** | 1.00 → **1.00** | 0.71 → **0.71** | 1.2 |
| hauler_1500 | 1.44 → **1.44** | 0.66 → **0.66** | 0.52 → **0.52** | 11.1 |
| sentinel_cruiser | 0.29 → **0.29** | 0.86 → **0.86** | 0.64 → **0.64** | 5.2 |
| sentinel_wolfhound | 0.26 → **0.26** | 0.88 → **0.88** | 0.72 → **0.72** | 5.0 |
| sentinel_bastion | 0.41 → **0.42** | 0.75 → **0.76** | 0.58 → **0.58** | 11.1 |
| junkyard_beater | 1.58 → **1.51** | 0.70 → **0.70** | 0.44 → **0.44** | 4.2 |
| tansy_13 | 0.50 → **0.49** | 0.81 → **0.81** | 0.70 → **0.70** | 5.1 |
| drayman_cargo | 0.69 → **0.69** | 0.68 → **0.68** | 0.52 → **0.52** | 8.6 |
| furrow_24 | 0.48 → **0.49** | 0.75 → **0.76** | 0.58 → **0.59** | 9.6 |

`tests/handling_balance.rs` passes. Ride (`tests/ride.rs`, kerbs / crests / slides / side hits)
passes; kerb strikes are unchanged (no brake, so no jacking; `vehicle/kerb_strike_*` goldens
match round12's).

**Roll and pitch damping of the touched cars** (lead request; `--example body_damping`: a
0.3 rad/s kick at rest, damping ratio from the first two extrema; "no overshoot" = ζ ≳ 0.9;
before = round12's suspension data, same code). More damping on every axis, no stiffer spring:

| car | roll before → after | pitch before → after |
|---|---|---|
| ironside_440 | 1.05 Hz ζ 0.28 → **1.06 Hz ζ 0.36** | 1.09 Hz ζ 0.35 → **1.11 Hz ζ 0.48** |
| furrow_24 | 1.50 Hz ζ 0.26 → **1.54 Hz ζ 0.34** | 1.41 Hz ζ 0.38 → **ζ ≈ 0.4–0.5** (fit reads 0.26 Hz ζ 0.39: the second extremum is a slow settle, not a cycle) |
| tansy_13 | 1.28 Hz ζ 0.46 → **no overshoot** | 1.41 Hz ζ 0.38 → **no overshoot** |
| junkyard_beater | 0.97 Hz ζ 0.41 → **no overshoot** | 1.10 Hz ζ 0.25 → **no overshoot** |
| sentinel_bastion | 1.48 Hz ζ 0.26 → **1.25 Hz ζ 0.44** | 1.27 Hz ζ 0.43 → **no overshoot** |
| lindwurm_4t (travel only) | 2.07 Hz ζ 0.54 → 2.07 Hz ζ 0.54 | 2.11 Hz ζ 0.51 → 2.11 Hz ζ 0.51 |

Roll is still ζ 0.34–0.44 on ironside / furrow / bastion: their bump ζ sits at 0.35–0.43 and the
ride test caps bump ζ at 0.45. Drift Lab W's ×1.4 bump / ×1.8 rebound (ζ ≈ 0.95) would need that
band (and its ARB ×0.25) changed roster-wide; that's W's / A's call, not this row's.

## 6. Tests and scenarios
- `crates/wp_vehicle/tests/brake_dive.rs`: every roster car, wheel (manual gears), keyboard and
  owner default (keyboard, manual gears, TCS / ESC off), 100 and 60 km/h: peak ≤ 2.55° (target
  ≈ 2.5°), steady ≤ 2.4°, still dives > 0.8°, front body > 4 cm, collider > 3 cm, never past the
  end of the travel, decel > 0.6 g; holding the brake into reverse ≤ 2.55°;
  `the_bump_stops_hold_the_nose_off_the_road` (geometry, every car > 1 cm).
- `crates/wp_sim/tests/brake_dive.rs`: the two-mode game-loop check above (fails on the old
  physics: vireo_xs already 2.50° with anti-dive off).
- `apps/simcli/scenarios/vehicle/brake_dive_100_{arcade,sim}.ron`: three soft cars, stop
  distance 38–65 m, no crush (pitch is in the wp_sim test: simcli has no pitch metric).
- `apps/testbed-web/e2e/brake_dive.mjs` (`npm run e2e:brake-dive`; not added to `npm run e2e`
  to keep that run's length; lead's call).

## Contract change requests
| # | Change | Why | Who is affected | Status |
|---|---|---|---|---|
| 1 | Optional `SuspensionDef::anti_dive_frac: Option<f32>` (front corners; default 0.55) and `anti_lift_frac: Option<f32>` (rear corners; default 0.4) | per-car geometry (trucks / old cars have less anti-dive, sports cars more); today they are `wp_vehicle` constants for every car | lead (contracts, schemas), H (car data), A (wp_vehicle reads them) | open |

## Changes outside wp_vehicle
- `crates/wp_wasm/src/lib.rs`: `WheelInfo::static_compression_m` (lead crate; additive JSON field).
- `crates/wp_vehicle/Cargo.toml`: `serde_json` dev-dependency (the `body_damping` example only).
- `apps/testbed-web`: `wasm/layout.ts` (`wheelCentreZ`), `wasm/sim.ts` (type), `vehicle_view.ts`,
  `render/fx/index.ts`, `e2e/brake_dive.mjs`, `package.json` (`e2e:brake-dive`).
- `data/cars/*.json`: 5 cars (above). Goldens + pinned hashes re-recorded in a `[bless]` commit.

## Known gaps / notes for others
- Anti-squat under acceleration is not modelled (drive torque jacks nothing).
- The chassis collider is still a box with its bottom 0.15 m over the full body length (the
  tall cars' drawn body is 20–22 cm up): a lower-than-drawn collider can scrape on crests; now
  ≥ 3.7 cm away under braking. A per-car floor from the lattice would match the drawn body.
- Lattice bottoms are 11–15 cm on the cars (real bumpers ~15–20 cm); not changed (B/H's data).
  lucciola_350 has only 1.2 cm left with its front struts on the bump stops (a hard kerb or
  landing can still squash the bump stop that far); a higher nose in its lattice/mesh would fix
  it without touching its ride.

## Requests
- **H (cars):** carry the six cars' data changes (§2) into the `*.spec.md` files and
  `VALIDATION.md`.
- **Lead (contracts):** request #1 above (per-car anti-dive / anti-lift).
- **Lead:** decide whether `e2e/brake_dive.mjs` joins `npm run e2e`.
- **W / A (suspension feel):** roll on ironside / furrow / bastion stays ζ 0.34–0.44 because the
  ride test caps bump ζ at 0.45 (§5); Drift Lab W's damping-up defaults need that band moved.
- **F (net), from the first pass:** `vesper_e2e::four_clients_three_minutes_in_port_vesper`
  failed there because racer 0 was captured inside the test's scripted disconnect of p1 (steps
  2700–2880), and events sent while a client is offline are never replayed on reconnect. It
  passes on the merged branch (timing moved), but the gap stays: a capture during a reconnect
  window is lost for that client.

## Checks (`lead/brake-dive` after merging `origin/lead/round12` @ 848465d3; CARGO_INCREMENTAL=0, no debug info)
- `cargo fmt --check`, `cargo clippy --workspace -- -D warnings`: clean. (`--all-targets` also
  flags `wp_audio/src/siren.rs:437` `12345.678_901` in a lib test — round12's code, untouched.)
- `cargo build --target wasm32-unknown-unknown -p wp_wasm`, `npm run wasm`, `tsc --noEmit`: ok.
- `npm run e2e:brake-dive` (headless Chromium, both modes): pass; lowest drawn front body vertex
  6.7 cm (beater), 7.8 cm (ironside), 12.7 cm (bastion); drawn dive 2.2–2.7° from the first
  (cruising, slightly nose-up) frame; tyres on the road ±0.4 cm.
- Goldens: reference reset to round12's (my earlier bless was on the old base), then `[bless]`
  (commits 3b85bbed, 1092d99d, 77120bdf). Kept unchanged vs round12: full-throttle launches
  (`vehicle_coupe_accel_*`), kerb strikes, frontal barrier / sports frontals, `golden_fwd_liftoff`,
  `golden_rwd_power_slide` — no brake, no jacking. Moved: everything that brakes (driver, ESC /
  TCS, AI / traffic stops) or drives one of the six re-tuned cars. After the bless
  `simcli goldens` matches all but `city_siren_traffic_no_siren` (its expectation, below).
- `simcli run apps/simcli/scenarios`: all pass except `city_siren_traffic_no_siren`
  (`civilian_collisions` 1): **identical on round12** — a run equal to round12 bit for bit
  (anti-dive off, round12 car data: hash 0d6d6e6c… = round12's golden) fails the same way
  (traffic agents 41 / 133 at traffic step 2208). Rear anti-lift 0.75 was tried and flipped
  `police_tunnel_escape_founders_cut{,_control}` (captured at 55.6 s), so it stays 0.6.
- `cargo test --workspace --no-fail-fast` (debug): 149 binaries ok. Failures, each checked on a
  clean `origin/lead/round12` checkout:
  - `simcli` `all_scenarios_pass…` / `goldens_match`: the siren expectation above (round12 too).
  - `wp_opponent race::wanted_ai_racer_flees_police_through_the_city`: **fails on round12 too**.
  - `wp_race race_picker::grid_highway_sprint`: **fails on round12 too**; `wp_wasm
    race_tests::short_sprint_…` (same grid sprint): **fails on round12 too**.
  - `wp_race race_picker::hollins_hill_sprint`: passes on round12, fails here — two AI racers
    DNF at gate 3 queued behind traffic (finish window), the pattern of the grid sprint. A
    1-minute AI-in-traffic race that braking changes reshuffle; L / N.
- Determinism: `vehicle/brake_dive_100_arcade` and `_sim` give the same hash; goldens re-run
  after each bless match.
