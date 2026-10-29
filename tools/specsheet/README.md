# tools/specsheet — car roster tooling (workstream 09)

Plain Python 3 (standard library only, 3.9+). Nothing here is reachable from the game or
`wp_wasm`; it produces and checks data.

| File | What it does |
|---|---|
| `roster_inputs.py` | **The source of truth** for the roster: per-car engineering inputs (mass, geometry, torque curve, gearing, tire sizes, ride frequencies, roll targets, caliper sizes, turning circle…), prose (design note, how it breaks, source types) and lattice options. |
| `build_roster.py` | Derives `data/cars/<id>.json`, `data/cars/<id>.spec.md` and `data/lattices/<id>.json` from the inputs. Lattices = workstream 03's `gen-lattice` output + the documented hand adjustments it cannot express yet (`adjust_lattice`). Deterministic. |
| `estimate.py` | Point-mass performance estimator (0-100, top speed, 100-0, skidpad, rough class index). Not the game's vehicle model. |
| `collect_results.py` | Runs `wp_vehicle`'s metrics example and `damage-barrier --sweep` for every roster car, checks the barrier expectations and writes `validation_results.json`. |
| `validation_results.json` | Measured sim results (generated; read by `specsheet.py`). |
| `specsheet.py` | Renders `data/cars/ROSTER.md` (roster table) and `data/cars/VALIDATION.md` from the car JSON and `validation_results.json`. |
| `smoke_sim.py` | Spawns every roster car in `wp_sim` via `simcli` (scenario format v2) in both damage modes, twice, and checks the state hashes match. |
| `pursuit_probe.py` | Police pursuit numbers through `simcli`: every car's 0-100 / 0-cap / 80-144 km/h times and a straight-line chase model against the cruiser (`--chase`, with `wp_police`'s speed cap and target margin). |
| `ram_probe/` (crate `ram-probe`) | `ram-probe [car…]`: repeated rams (frontal / offset by the car, side / rear taken) on **one** damage model in both modes — health after each ram, the ram that drops it below `wp_police`'s disengage health (0.4) and the first total-loss ram. `vmax-trace`: `wp_vehicle`'s top-speed run stopped before the rig's 10 km ground slab ends (the metric's own run falls off the world on slow cars); `collect_results.py` takes Vmax from it. | Barrier checks read the `extreme` rows (contract v0.4.0 renamed the old Simulation to Extreme); "Arcade milder" compares with `sim` (the old Arcade). |

## Typical loop

```sh
cargo build --release -p wp_damage --bins -p wp_vehicle --example metrics -p simcli -p ram-probe
# edit tools/specsheet/roster_inputs.py, then:
python3 tools/specsheet/build_roster.py          # cars, spec sheets, lattices
python3 tools/specsheet/collect_results.py       # sim metrics + barrier sweeps → validation_results.json
python3 tools/specsheet/specsheet.py             # ROSTER.md + VALIDATION.md
python3 tools/specsheet/smoke_sim.py             # loads + determinism through the sim
python3 tools/specsheet/pursuit_probe.py --chase # police cruiser vs every car (acceleration / chase)
./target/release/ram-probe sentinel_cruiser      # cruiser durability under repeated rams
cargo test -p wp_contracts                       # schema + loader validation of all data
```

Never hand-edit the generated files; the next build overwrites them.

## Derivations (summary — each spec sheet shows the numbers)

- `mass_kg` = curb (full tank) − half a tank (0.745 kg/L) + 75 kg driver; `fuel_capacity_l`
  set per car (contract v0.2.0: fuel in `mass_kg` at 50 %).
- Automatics carry `torque_converter` (stall rpm, stall ratio, coupling rpm); on-demand AWD
  cars use `AwdOnDemand` (the estimator assumes the coupling closed at launch).
- Inertia: box formula `m·(a²+b²)/12` per axis × mass-concentration factor `k` (0.75 mid-engine
  … 0.97 long-overhang/body-on-frame).
- CdA = Cd × `area_k`·W·H (+ police lightbar 0.05 m²).
- Tire radius from the ISO size code; loaded = 0.97 × unloaded (0.96 truck/SUV).
- Springs `k = m_s·(2πf)²`; dampers `c = 2ζ√(k·m_s)`, rebound 1.5 × bump.
- Anti-roll bars: total roll stiffness for a target roll gradient (°/g) with a front share;
  springs contribute `k·t²/2`, bars `k_arb·t²` (the bar rate in `arb_n_per_m`).
- Brakes: disc `T = 2·μ_pad·p·A_piston(one side)·r_eff`, drum `T = BF·p·A_wc·r`; line pressure
  100 bar modern, 65 bar 70s. `wp_vehicle` gives the axle `bias_front` favours its full
  `max_torque_*` and scales the other by (1 − b)/b, so `max_torque_*` = caliper capacity per
  wheel and `bias_front` is the proportioning valve: 0.5 when the rear caliper is the limit,
  above 0.5 only to trim an oversized rear to the front's lock margin
  (`ideal rear = T_f · lock_r / lock_f` at 1 g). **ABS ceiling**: delivered front torque is capped
  at 1.5 × the front lock torque on the car's own tire (rear scaled with it) because
  `wp_vehicle`'s ABS cannot modulate an oversized caliper — remove once it can.
- Steering lock from kerb-to-kerb turning circle (mean road-wheel angle `atan(L/R_rear)`).
- Optional per-car knobs (wave 3): `downforce=(front, rear)` → `aero_downforce_coeffs` (wings,
  splitters; on top of the body's `lift`), with `downforce_why`; lattice `cell_scale` multiplies
  every passenger-cell beam section (roll cage, carbon tub, tube spaceframe) with `cell_why`;
  lattice `nose_scale` multiplies the front-rail sections (crash nose / crash boxes) with
  `nose_why`. Each is written into the spec sheet. Era `2020s` = the 2010s HSS cell.
- Engine braking = 10-12 % of peak torque; clutch capacity 1.35-1.6 × peak torque.
- Stock shift time (`shift_time_s`, owner playtest row 97, 2026-10-20): manual sports 0.42-0.45 s,
  manual economy / hot hatch 0.45-0.50, muscle manual 0.50, light-truck manual 0.65, beater
  0.70, dual-clutch 0.18. Converter automatics are still 0.30-0.40 s: their shift model gets
  *faster* with a longer shift (bug reported to 02, `docs/integration/cars.md` row 97); planned
  0.60-0.70 (police packages 0.45-0.55) once that is fixed. Mods scale
  it (`drivetrain.shift_time_scale`; the best tier of each line multiplies, ×0.28 fully modded,
  so a sports manual lands at ≈ 0.12 s); `VALIDATION.md` lists stock → fully modded per car.

## Tire compounds

Each car names an *intended* compound (`street`, `sport`, `police_pursuit`, `all_terrain`,
`vintage_bias`; the last two do not exist yet). `build_roster.py` uses it when `data/tires/<compound>.json` exists, else
`street`. The estimator then scales street peak friction by `estimate.COMPOUND_SCALE`
(sport 1.10, semi-slick 1.22, police pursuit 1.04, all-terrain 0.90, vintage bias-ply 0.85 —
rule-of-thumb ratios of peak lateral g between tire categories on the same car).

## Tests

The data itself is tested by `cargo test -p wp_contracts` (schema + loader validation) and
`cargo test -p simcli`. The scripts are checked by running them: each exits non-zero on
failure.

## Known gaps

- The estimator is a point-mass model: no wheelspin dynamics, no torque converter, no
  combined slip, no transient handling. Use it to catch data mistakes, not to tune feel.
- `adjust_lattice` recognises beam roles in gen-lattice output by section size (`SEC` mirrors
  `lattice_gen::DEFAULT_SECTIONS`); if 03 changes those sections, update `SEC` or the era
  adjustment stops matching (the spec sheets' change log would then show fewer beams).
- The remaining hand adjustments are listed in `docs/integration/cars.md`.

## Perf

Building all 21 cars (including 21 gen-lattice runs) takes ≈ 1 s; `collect_results.py`
≈ 15 s; `specsheet.py` < 1 s. Roster lattices: 244-332 nodes and 1536-2138 beams per car.
