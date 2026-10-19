# Car roster integration notes

- **Workstream / prompt:** 09 — car roster data
- **Branch:** `ws/cars`
- **Crates / folders owned:** `data/cars/` (roster files), `data/lattices/` (roster car files, not the generator), `tools/specsheet/`
- **Contract version built against:** 0.1.0

## Exposes

- 13 cars, each `data/cars/<id>.json` (VehicleDef) + `data/cars/<id>.spec.md` + `data/lattices/<id>.json`:
  racers `vireo_xs`, `kestrel_ts`, `shiro_turbo`, `hakone_gt`, `rampart_50`, `ironside_440`,
  `sperber_32`, `lindwurm_4t`, `lucciola_350`, `hauler_1500`; police `sentinel_cruiser`
  (PoliceCruiser), `sentinel_wolfhound` (PoliceInterceptor), `sentinel_bastion` (PoliceSuv).
  All load via `wp_contracts::load_car` and validate against `schemas/`.
- `data/cars/ROSTER.md` — roster table (mass, power, kW/t, est. 0-100 / Vmax / skidpad, class
  index estimate, durability, price) for design docs.
- `data/cars/VALIDATION.md` — class targets, estimates, pass/fail, sim columns (pending), barrier matrix.
- `tools/specsheet/` — `build_roster.py` (regenerate everything from `roster_inputs.py`),
  `specsheet.py` (tables), `estimate.py` (point-mass estimator), `nameplate_check.py`
  (naming policy, exit 1 on hit), `smoke_sim.py` (every car through `simcli`, both modes,
  determinism). See `tools/specsheet/README.md`.
- Police push bar convention: extra `FrontBumper` nodes ahead of the bumper, detachable
  **group 3** (`break_impulse_ns` 15000, 25 kg). Economy/repair can identify it by group id.

## Needs

| From | What | Workaround now |
|---|---|---|
| 02 vehicle | `data/tires/sport.json`, `police_pursuit.json` (planned in prompt 02) | Cars name the intended compound; JSON uses `street` until the file exists. Re-run `build_roster.py` after it lands — it switches automatically. |
| 02 vehicle | Two more compounds: **`all_terrain`** (Hauler; ≈ 0.90 × street peak μ, softer, more rolling resistance) and **`vintage_bias`** (Ironside; ≈ 0.85 × street, low cornering stiffness, early falloff) | Same fallback to `street`. |
| 02 vehicle | `street.json` lateral `d = 1.0` reads like a summer tire: the light Vireo estimates 0.95 g on it (class: 0.82-0.92). Consider `d ≈ 0.90` for street, with `sport` ≈ 1.0-1.1. | None; noted in VALIDATION.md. |
| 02 vehicle | Metric scenarios (0-100, 100-0, skidpad, top speed) + `wp_vehicle::metrics` | `estimate.py`; VALIDATION.md sim columns read "pending". |
| 03 damage | `gen-lattice` with body profiles (see below), barrier scenarios, `uhss` material | `gen-box-lattice` + scripted hand adjustments; UHSS emulated with larger `hss` beam areas. |
| 08 economy | Mods: Kestrel AWD conversion (`layout → Awd{front_split: 0.6}` + centre diff), Hauler 4WD (`Awd{0.4}`), Ironside clutch LSD + disc-brake conversion, Rampart/others' tire upgrades. | Not created (data/mods is 08's). |
| 10 harness / lead | Run the roster through the metric + barrier scenarios on the integrated build; write results to `tools/specsheet/validation_results.json` (format in VALIDATION.md); run `nameplate_check.py` in CI. | `smoke_sim.py` proves load + determinism on the stubs. |

## Contract change requests

| # | Change (type / field / variant) | Why | Who is affected | Status |
|---|---|---|---|---|
| 1 | `DrivetrainDef::torque_converter: Option<TorqueConverterDef { stall_rpm, stall_torque_ratio, coupling_rpm }>` (None = manual/DCT clutch) | 5 of 13 cars are automatics (Ironside, Hauler, all three police). Without converter multiplication their launches are 0.5-1 s slow (Hauler estimate 8.4 s vs ≈ 7.5 s class figure) and they stall-launch like manuals. | 02, 08 (gearbox mods), 06 (AI launch) | proposed |
| 2 | `DriveLayout::AwdOnDemand { base_front_split, max_rear_split, engage_slip }` (or a `coupling` field on `Awd`) | On-demand clutch-pack AWD (Lindwurm, Wolfhound) is front-drive until the front slips; a fixed split misses the lift-off/power-on character. Using fixed 65/35 and 55/45 meanwhile. | 02, 08 | proposed |
| 3 | `BrakeDef::rear_kind: Disc \| Drum` and a thermal/fade model input (`thermal_mass_kg`, `fade_start_c` per axle) | Ironside's "scary brakes" are only partly expressible: low capacity is modelled (0.79× front lock torque), drum fade is not. Also useful for police pursuit fatigue. | 02, 08 (brake mods) | proposed |
| 4 | Clarify `BrakeDef` semantics in the doc comment: are `max_torque_front/rear_nm` hydraulic capacities at full pedal (what the roster assumes), with `bias_front` the pedal split, or are they pre-scaled by bias? | The current wording ("at full pedal and `bias_front = 1`") lets 02 implement it two ways; the roster data assumes capacity-per-wheel and bias = pedal force split. | 02 | proposed (doc only) |
| 5 | `SuspensionDef::axle: Independent \| Solid` (per axle) | Live/solid rear axles (Ironside, Hauler, Bastion) couple left/right wheels (axle tramp, roll steer, high unsprung mass). Currently modelled as independent corners with high unsprung mass. | 02, 03 (axle tear-off) | proposed |
| 6 | `VehicleDef::roll_center_height_m: [f32; 2]` | ARB rates were derived assuming 0.08/0.12 m roll centres; without geometry the sim's roll response differs from the spec sheets' roll gradients. | 02 | proposed (low) |
| 7 | `BodyRegion::PushBar` (or a generic `Accessory`) | Push bar currently uses `FrontBumper`, so HUD/repair cannot show or bill it separately (group 3 is the only handle). Prompt 08's body-kit pieces need the same. | 03, 08, 04 | proposed |
| 8 | `VehicleDef::fuel_capacity_l` (+ fuel mass in `mass_kg` at a stated fill) | Fuel-leak effects (`HandlingEffects::fuel_leak_rate`) need a tank size to drain. | 02, 03, 08 | proposed (low) |

Non-contract data requests: `data/materials/uhss.json` (03: yield ≈ 1000-1200 MPa, failure strain ≈ 0.08) — the 2010s cabins currently use `hss` with 1.4-1.6× area.

## What `gen-lattice` should learn (for prompt 03's owner)

Every item below is a hand adjustment `tools/specsheet/build_roster.py::adjust_lattice` makes
to `gen-box-lattice` output today. Each spec sheet ends with the exact list applied to that car.

1. **Body profiles** — top-layer heights per row (hood/cowl/deck at beltline, greenhouse at
   roof height) and tumblehome (roof rows ≈ 86 % width). Profiles used: sedan, coupe, hatch,
   mr, truck, suv (`PROFILE_TOP`). The box grid has the hood at roof height, which makes nose
   crush and bonnet-over-bumper geometry wrong.
2. **Pillars are structure, not glass.** The box generator classes side-door↔windshield beams as
   glass, so the A-pillar is a 1e-4 m² glass beam. Pillar/roof-rail beams need their own
   material class.
3. **Era presets** for cabin / pillar / rail / sill material and area (70s mild steel, 90s,
   2000s, 2010s UHSS-ish). Table `ERA` — this is what makes Simulation lethality differ by era.
4. **Crumple resolution.** Only two node rows (7-8) sit between bumper and firewall, so a rail
   gets one soft and one hard segment. Need ≥ 3-4 rows in the nose and tail for a believable
   soft→hard crush curve and the 0.6 m @ 56 km/h calibration.
5. **Separate radiator and engine nodes.** The box generator mounts both on row 8, so they
   fail together; the bumper → radiator → engine ordering test needs them on different rows.
6. **Layout-aware component placement**: FF transverse (engine + gearbox side by side), FR
   longitudinal (gearbox in the tunnel, prop shaft), MR (engine bay behind the cabin, radiators
   in the nose, side fuel tanks), truck (side-saddle tank outboard of the frame rail), 70s
   (tank behind the rear axle). Engine-node mass concentration (×2.5 here).
7. **Mid-engine regions**: engine bay rows behind the firewall must not be `Cabin` (otherwise a
   rear hit counts as cabin intrusion and totals the car immediately).
8. **Ladder frame** (truck/SUV profiles): two full-length inner bottom node lines as
   `FrameLeft`/`FrameRight` with large-area rails (1.2e-3 m²), cross-members (6e-4) and
   diagonals; frame-mounted suspension tolerates more intrusion (0.08 m vs 0.05).
9. **Pickup bed** regions (bed floor/tailgate as `Trunk`, bed sides as quarters) and a cab
   that ends at the bed.
10. **Accessories / detachable extras**: push bar = extra nodes 0.10 m ahead of the bumper,
    bolted through to the rail nodes, own detach group. Body-kit pieces (prompt 08) need the
    same hook: a `--accessory pushbar` style option with a group id convention.
11. **Doors as detachables**: there are only 1-2 nodes per door region, so doors cannot hinge
    or detach; needs ≥ 2×2 nodes per door and hinge lines on the A/B pillars.
12. **Per-car overrides** that should be generator parameters: steel vs plastic bumpers
    (impulse 7000-9000 vs 3000 N·s), aluminium vs steel hood/panels, live-axle mount weakness
    (rear suspension fail 0.03 m, wheel 0.06 m), sacrificial aluminium rear subframe (MR).
13. Suggested CLI: `gen-lattice --profile <p> --era <e> --layout <ff|fr|mr|awd> --frame
    <unibody|ladder> --bumpers <plastic|steel> --pushbar --dims W L H --wheelbase --clearance
    --mass`.

## Changes outside my crates

- `data/cars/README.md` "Current files" section updated with the roster (I own `data/cars/`).
- No code changes to any crate. Nothing added to `wp_contracts`, `schemas/`, `data/tires/`,
  `data/materials/`, `data/mods/`, `apps/simcli/` or the justfile.

## Perf numbers

| Measurement | Machine | Native | Browser | Budget |
|---|---|---|---|---|
| Lattice size per roster car | — | 120-124 nodes, 704-714 beams | same | prompt 03: 0.25 ms/car lattice step |
| `build_roster.py` (13 cars incl. generator runs) | cloud container | 0.44 s | — | — |
| `simcli` smoke run per car (stub sim, 14 s simulated) | cloud container, release | < 0.1 s | — | — |

## Known gaps

- **Sim validation pending.** `wp_vehicle`/`wp_damage` are stubs on this branch; metric and
  barrier columns in VALIDATION.md read "pending". The Arcade and Simulation smoke hashes are
  identical because the stub damage does nothing.
- **Estimate misses** (point-mass model, see VALIDATION.md): Rampart 0-100 5.6 s vs 4.5-5.0
  (traction-limited; depends on `sport` longitudinal μ), Hauler 0-100 8.4 s vs 7.0-8.0 (no
  torque converter, 2WD), Vireo/Kestrel/Shiro skidpad above band (street/sport μ
  assumptions), Lindwurm 100-0 0.3 m over. Car data was deliberately not bent to fit the
  estimator.
- Intended tire compounds not yet available → all cars run on `street`.
- AWD on-demand, automatics, live axles, drum fade are approximated (contract requests 1-3, 5).
- Lattices are 120-node grids; damage *detail* (door hinges, multi-stage crumple) waits for
  `gen-lattice`. When it lands, swap the generator call in `build_roster.py::run_generator`
  and drop the adjustments it covers.
- Prices and durability classes are first-pass; 08 should tune prices against the economy.

## Lead feedback (2026-10-19)

**Merged** (`7dbf8f3`). `nameplate_check.py`: 54 files, 0 problems. All 13 cars load, validate and run
deterministically in both modes on the **real** `wp_vehicle` model (`smoke_sim.py` green).

Lead change in your area: `tools/specsheet/smoke_sim.py` now writes simcli scenario **format v2**
and calls `simcli run ... --no-write` (the harness changed the format and CLI). Pull it.

First integrated numbers (`cargo run --release -p wp_vehicle --example metrics`, cloud container;
most cars are still on `street` because they fell back to it):

| car | 0-100 s | 100-0 m | skidpad g | Vmax km/h |
|---|---|---|---|---|
| vireo_xs | 9.87 | 45.7 | 0.89 | 202 |
| kestrel_ts | 7.41 | 46.3 | 0.86 | 247 |
| shiro_turbo | 6.68 | 42.4 | 0.88 | 258 |
| hakone_gt | 6.16 | 40.5 | 0.83 | 283 |
| rampart_50 | 6.29 | 55.6 | 0.81 | 270 |
| ironside_440 | 7.18 | 61.4 | 0.74 | 212 |
| sperber_32 | 6.34 | 41.9 | 0.84 | 274 |
| lindwurm_4t | 5.53 | 43.9 | 0.86 | 260 |
| lucciola_350 | 4.94 | 42.8 | 0.87 | 290 |
| hauler_1500 | 8.95 | 49.8 | 0.67 | 204 |
| sentinel_cruiser | 8.74 | 57.0 | 0.77 | 236 |
| sentinel_wolfhound | 6.36 | 61.7 | 0.80 | 243 |
| sentinel_bastion | 8.47 | 51.3 | 0.66 | 213 |

Next for you:
1. `data/tires/` now has `sport`, `semi_slick`, `police_pursuit`, `wet_biased` → re-run
   `build_roster.py` so cars switch to their intended compounds, then fill the "sim" columns of
   `VALIDATION.md` and write `tools/specsheet/validation_results.json` from the metrics example.
2. Look at: Rampart 0-100 6.3 s vs 4.5-5.0 band (traction, as you predicted); Wolfhound 100-0
   61.7 m is long for an interceptor; Ironside 61.4 m is the intended "scary brakes".
3. ⚠ Brake semantics: 02 implemented "biased axle gets full `max_torque_*`, the other × (1−bias)/bias".
   Your data assumed capacity per wheel with bias = pedal split. 02 will add a worked example to its
   README; re-check your brake numbers against it.
4. Economy (08) asks for a floor car `junkyard_beater` (Beater class, base price 3 000).
5. barrier/crash columns wait on 03 (damage) — the sim now delivers contacts to damage.

Contract requests (decisions; implemented together in v0.2.0 unless noted):
| # | Decision |
|---|---|
| 1 torque converter | Accepted (`DrivetrainDef::torque_converter: Option<...>`). |
| 2 AWD on-demand | Accepted, merged with 02 #5. |
| 3 rear drum + fade | Deferred (post Checkpoint 1; brake-fade model is new vehicle work). |
| 4 brake doc | Accepted, doc only — see item 3 above. |
| 5 solid axle | Deferred (post Checkpoint 2). |
| 6 roll centre | Deferred (low). |
| 7 `BodyRegion::PushBar`/`Accessory` | Accepted in principle as a generic `Accessory` region; final shape waits on 03's region list. |
| 8 `fuel_capacity_l` | Accepted, `Option<f32>`, fuel mass included in `mass_kg` at 50 % fill. |
