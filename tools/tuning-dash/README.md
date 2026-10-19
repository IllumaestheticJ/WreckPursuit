# tools/tuning-dash

**Owned by workstream 20 (machine Q).** Parameter sweeps over any data field → plots and
pass/fail tables in a local web page → promote the chosen values back into `data/` with a
changelog line. How a tuning session runs end to end: [`docs/TUNING.md`](../../docs/TUNING.md).

```sh
tools/tuning-dash/tune.py presets                       # the standard sweeps
tools/tuning-dash/tune.py sweep spring_damper           # run one (preset name or spec path)
tools/tuning-dash/tune.py serve                         # dashboard → http://127.0.0.1:8765
tools/tuning-dash/tune.py show spring_damper --metrics step_steer_overshoot_pct
tools/tuning-dash/tune.py promote spring_damper p0009 --note "less overshoot" [--dry-run]
tools/tuning-dash/tune.py diff car:debug_sedan car:debug_coupe
```

`just tune <spec>`, `just tune-dash`, `just tune-test` wrap the same commands. Needs Python
≥ 3.10 (stdlib only) plus PyYAML for `.yaml` specs (`pip install pyyaml`; `.json` specs need
nothing), and cargo (the probe is built on first use).

## How it works

| piece | what |
|---|---|
| `probe/` (Rust, `tuning-probe`) | one sweep point per process: reads a job JSON, applies overrides to an **in-memory overlay** of the data root (disk never changes while sweeping), runs one measurement, prints a result JSON. Also `diff`, `resolve`, and `promote` (surgical text edits + contract-loader validation + changelog). |
| `tuning_dash/spec.py` | sweep spec (YAML/JSON) → list of points |
| `tuning_dash/runner.py` | runs points in parallel (`workers`), caches unchanged points, writes `out/sweeps/<name>/sweep.json` |
| `tuning_dash/server.py` + `web/` | stdlib HTTP server + a no-build HTML/JS page (inline SVG charts) |
| `presets/` | the standard sweeps from prompts 02/03 plus comparisons |

Probe kinds (`probe.kind` in a spec):

| kind | runs | metrics |
|---|---|---|
| `scenario` | a simcli v2 scenario (`file`, optional `systems`) through `simcli::run` | `<entity>.<metric>` for every simcli summary metric, `passed`; checks = the scenario's `expect` (+ `expect_pending` as pending); simcli outputs (`metrics.csv`, `events.jsonl`, `hashes.txt`, `summary.json`, `replay.bin`) and a top-down path thumbnail per point |
| `vehicle_metrics` | `wp_vehicle::metrics` on `car` (`metrics: [zero_to_100, braking, skidpad, step_steer, lift_off, power_on, power_oversteer, top_speed]`, default all but `top_speed`; `skidpad_radius_m` 50, `turn_radius_m` 40) | `zero_to_100_s`, `braking_100_0_m`, `skidpad_max_lat_g`, `understeer_deg_per_g`, `step_steer_overshoot_pct`, `lift_off_yaw_rate_delta_rps`, `lift_off_curvature_ratio`, `power_on_curvature_ratio`, `power_oversteer_throttle`, `top_speed_kmh` |
| `barrier` | `wp_damage::barrier::run_barrier` for `car`, `mode` (`Arcade`/`Simulation`), `impact` (`Frontal`/`Offset`/`Side`/`Rear`) at `speeds_kmh` (default 15/25/40/56/80; `other_mass_kg` defaults to the car's mass for `Side`) | series vs speed + flat `<metric>@<speed>` for `crush_m`, `cabin_intrusion_m`, `plastic_energy_kj`, `health`, `broken_beams`, `detached_parts`, `failed_components`, `repair_total`, `total_loss`, `power_scale`; failed components per speed in `info` |
| `barrier_scenario` | a `crates/wp_damage/scenarios/*.ron` file with its own checks | as `barrier`, plus pass/fail of every scenario check |

## Spec reference

```yaml
name: tire_lateral_b            # output folder out/sweeps/<name>/ (default: file stem)
description: one line for the dashboard
data: data                      # data root (default data/)
probe: {kind: vehicle_metrics, car: debug_sedan, metrics: [skidpad, step_steer]}
params:                         # swept parameters
  - target: "tire:{tire}:lateral.b"   # <file>:<field path>; {var} filled from the variant
    targets: [...]                     # or several targets moved together by the same value
    label: lateral B                   # axis/column name (default: last path part)
    op: scale                          # set (default) | scale | add
    values: [0.8, 0.9, 1.0, 1.1, 1.2]  # or range: {from: 0.8, to: 1.2, steps: 5} / {..., step: 0.1}
    round_sig: 6                       # scale/add results rounded to N significant digits (default 6)
combine: grid                   # grid (cartesian) | zip (same length) | each (one param at a time)
variants:                       # comparison sides, crossed with the params (default: one "base")
  - {name: sedan, vars: {tire: street}, probe: {car: debug_sedan}}
  - name: coupe
    vars: {tire: sport}
    probe: {car: debug_coupe}
    set: [{target: "car:debug_coupe:mass_kg", value: 1300}]   # fixed overrides for this side
    expect: [{metric: zero_to_100_s, min: 5, max: 6}]          # checks for this side only
overrides: []                   # fixed overrides for every point
expect: [{metric: skidpad_max_lat_g, min: 0.8, max: 1.0}]      # checks on flat metrics, all points
plots: [skidpad_max_lat_g]      # charts shown first (barrier: "crush_m@56" also picks the speed)
workers: 4                      # parallel probes (default: CPU count, max 8)
```

**Targets.** File part: `cars/x.json` (any data-root path) or the shorthands `car:x`,
`tire:x`, `lattice:x`, `material:x`, `mode:sim` / `mode:arcade`, or `scenario` (the probe's
own simcli scenario, e.g. `scenario:entities[*].car`; never promoted). Field path: dotted keys
with selectors `[2]`, `[0,1]`, `[*]`, `[field=value]`, e.g.
`car:debug_sedan:suspension[0,1].spring_n_per_m` (front axle),
`lattice:debug_sedan:beams[material_id=hss].area_m2`. Integer fields stay integers.
`tuning-probe resolve <target>` lists what a target selects before you sweep it.

## Outputs

```
out/sweeps/<name>/sweep.json            spec, points, metrics, series, checks, applied changes
out/sweeps/<name>/points/pNNNN/job.json  exact probe job (re-run: target/release/tuning-probe run job.json)
out/sweeps/<name>/points/pNNNN/result.json
out/sweeps/<name>/points/pNNNN/<scenario>/{metrics.csv,…,replay.bin}   (scenario probes)
```

A point is re-measured only when its `job.json` changes (or with `--force`), so widening a
range re-runs only the new points. Results record every concrete change as
`{file, pointer, old, new}`; `promote` writes exactly those `new` values.

## Promote

`promote <sweep> <point>` (or the dashboard button) writes the point's changed values into the
data files by replacing only the value text (layout, key order and TOML comments are kept, so a
promote is a one-line-per-value diff), validates each file with the `wp_contracts` loader for
its folder, and appends one line per file to `docs/TUNING_CHANGELOG.md`. It refuses when a file
no longer holds the value the sweep started from (stale sweep; `--force` overrides), when the
result would not load, and for scenario overrides. Nothing is committed: review the diff and
commit it on the owning workstream's branch (see `docs/TUNING.md`).

## Tests

```sh
cargo test -p tuning-probe                                 # paths, overrides, text edits, promote, diff
python3 -m unittest discover -s tools/tuning-dash/tests     # specs, presets, end-to-end sweep + promote
```

The end-to-end Python test runs a real barrier sweep and a simcli scenario point against a
temporary copy of `data/` and promotes into that copy.

## Perf (cloud container, 4 cores, release probe)

| sweep | points | wall |
|---|---|---|
| `tire_lateral_bcde` (skidpad + step steer + lift-off, 2 cars) | 40 | 2.7 s |
| `spring_damper` | 30 | 2.1 s |
| `barrier_speeds` (17 speeds × 8 variants) | 8 | 0.8 s |
| `impulse_to_lattice_scale` | 12 | 0.3 s |
| `sedan_brakes` (simcli scenario, recorded) | 6 | 0.13 s |

One `vehicle_metrics` point with every manoeuvre but top speed ≈ 0.3 s; a barrier point at
five speeds ≈ 0.1 s. Process start-up and data loading are a few ms per point.

## Known gaps

- Thumbnails are top-down paths from `metrics.csv`, not rendered replay frames (the replay
  itself is linked; open it in the testbed).
- `barrier_scenario` copies the car/tire/lattice/material/mode folders to a scratch folder per
  point because `wp_damage::barrier::run_scenario` reads from a data root on disk.
- `DamageTuning` constants (solver iterations, buckle factors, …) are Rust code, not data, so
  they cannot be swept; only `data/` files and the scenario can.
- `combine: each` assumes the neutral value (`scale 1` / `add 0`) is one of the values when you
  want a base point in every line.
- Promote changes `data/` on your working tree only; data files belong to their workstreams
  (cars → 09, tires → 02, modes/materials → 03), so the commit goes through them.
