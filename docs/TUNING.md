# Tuning sessions: from sweep to promoted values

How to change a physics or damage number on purpose, with evidence, using
`tools/tuning-dash` (reference: [`tools/tuning-dash/README.md`](../tools/tuning-dash/README.md)).
A sweep never edits `data/`; only an explicit **promote** does, one reviewed line at a time.

## 1. Pick the question and the measurement

| question | probe | preset to start from |
|---|---|---|
| tire grip / balance (Magic Formula B, C, D, E) | `vehicle_metrics` (skidpad, step steer, lift-off) | `tire_lateral_bcde`, `tire_longitudinal_bcde` |
| ride / transient response (springs, dampers, ARBs) | `vehicle_metrics` | `spring_damper`, `arb_balance` |
| drivetrain losses, wheel inertia (optional v0.2.0 fields) | `vehicle_metrics` | `drivetrain_options` |
| how hard a crash bites (mode tables) | `barrier` | `impulse_to_lattice_scale`, `plasticity_scale` |
| crush / damage vs speed, both modes | `barrier` | `barrier_speeds` |
| does a change still pass the calibration checks | `barrier_scenario`, `scenario` | `damage_scenarios_plasticity`, `sedan_brakes` |
| how two cars (or modes) differ | any, with `variants` | `compare_sedan_coupe`, `barrier_speeds` |

`tools/tuning-dash/tune.py presets` lists them with point counts. Every preset runs in a few
seconds on a laptop-class CPU.

## 2. Write (or copy) a sweep spec

Copy the closest preset to `tools/tuning-dash/presets/<yours>.yaml` (or anywhere) and edit:

- `params`: the fields to move. Prefer `op: scale` with values around `1.0` (keeps the
  current value as the base point and works across corners/cars). Move fields that belong
  together with `targets: [...]` (damper bump + rebound). Use selectors for axles:
  `suspension[0,1]` front, `suspension[2,3]` rear.
- `combine`: `each` for a sensitivity scan (one field at a time), `grid` to see an
  interaction (keep it under ~50 points), `zip` for hand-picked combinations.
- `variants`: the comparison you care about (FWD vs RWD car, Arcade vs Simulation, impact
  kinds). Each variant can bring its own `expect` ranges.
- `expect`: the target from the design doc as a **range** (as simcli scenarios do), e.g.
  prompt 03's "56 km/h frontal ≈ 0.6 m crush and TotalLoss" →
  `{metric: "crush_m@56", min: 0.45, max: 0.75}` and `{metric: "total_loss@56", min: 1, max: 1}`.

Check the spec without running it: `tune.py sweep my.yaml --dry-run` prints every point and
override. `target/release/tuning-probe resolve car:debug_sedan:suspension[*].spring_n_per_m`
prints what a target selects.

## 3. Run it

```sh
tools/tuning-dash/tune.py sweep my.yaml          # or press "Run sweep" in the dashboard
```

Points run in parallel (`workers`), each in its own probe process against an in-memory copy of
the data with that point's overrides. Results land in `out/sweeps/<name>/`. Re-running after a
spec edit only measures new or changed points.

## 4. Read the dashboard

`tools/tuning-dash/tune.py serve` → http://127.0.0.1:8765, pick the sweep.

- **Metrics vs parameter**: one small chart per metric, x = the swept field, one line per
  variant (or per value of a second param). Barrier sweeps can switch x to *closing speed*
  (crush vs speed, one line per parameter value) and pick which speed the flat metrics use.
  Pin a dimension when more than eight lines would show. Hollow markers fail a check; hover
  for values, click to select the point.
- **Points**: every point with its values, pass/fail (with the failing checks), path
  thumbnails for scenario probes; sortable.
- **Point**: what the point changes (file, pointer, file value → point value), each check,
  simcli outputs, and the promote form.
- **Compare A / B**: two points side by side (default: the base points of the first two
  variants, i.e. two cars or two modes), every metric with the delta, plus an overlay of
  the metric vs speed for barrier probes.
- **Data file diff**: field-by-field diff of any two data files (arcade vs sim table,
  sedan vs coupe, street vs sport tire).

Read the shape, not one number: a value that only passes at a knife edge or a metric that
peaks exactly at the current value usually means the measurement, not the car, is the limit.
Narrow the range around the candidate and re-run before deciding.

## 5. Promote

When a point is the one you want:

1. Click **Preview** (or `tune.py promote <sweep> <point> --dry-run`): shows the files and the
   changelog line, checks nothing went stale and the files still load.
2. Write a note (why, which target it meets) and click **Promote** (or drop `--dry-run`).
   Only the value text changes in each file; the line goes to `docs/TUNING_CHANGELOG.md`:
   `- 2026-10-19 · cars/debug_sedan.json: /suspension/0/damper_bump_ns_per_m 2140.0 → 2782.0, … · from sweep spring_damper point p0009 (springs=1.0, dampers=1.3) · less overshoot`
3. Re-run the sweep (its old points now compare against the new base) and the checks that
   pin behaviour:
   ```sh
   cargo test --workspace            # data validation, wp_vehicle/wp_damage ranges, simcli goldens
   just goldens                      # golden hashes WILL change for any physics data change
   ```
4. Hand it to the owner. Data files belong to workstreams (tires → 02, modes/materials →
   03, cars/lattices → 09). Commit the data change plus the changelog line on (or into) that
   workstream's branch, regenerate goldens with `just bless`, and commit with `[bless]` in the
   message (the CI bless guard requires it). Mention the sweep name so the reviewer can re-run
   it: `tools/tuning-dash/tune.py sweep <name>`.

Promote refuses when the file no longer holds the value the sweep started from (someone changed
it since: re-run the sweep), when a value would make the file fail to load, and for scenario
overrides (scenarios belong to their owners).

## Rules of thumb

- Tune one layer at a time: tire coefficients before suspension, Simulation table before
  Arcade (Arcade is defined relative to Simulation in `data/modes/arcade.toml`).
- Keep the variants in every sweep that the change affects: a tire tweak for the sedan's
  `street` tire also changes every other car on `street` tires; add them as variants.
- Expected ranges, not exact numbers, belong in specs (the goldens pin exact behaviour).
- Determinism is unaffected: overrides are plain data, and the same job gives the same result
  hash (`final_hash` / barrier `hash` in a point's info).
