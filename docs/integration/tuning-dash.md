# Tuning dashboard integration notes

- **Workstream / prompt:** 20 — tuning dashboard (machine Q)
- **Branch:** `ws/tuning-dash`
- **Crates / folders owned:** `tools/tuning-dash/` (Rust crate `tuning-probe` in
  `tools/tuning-dash/probe`, Python `tuning_dash`, `web/`, `presets/`), `docs/TUNING.md`
- **Contract version built against:** 0.1.0

## Exposes
- `tools/tuning-dash/tune.py {sweep, presets, list, show, diff, promote, serve}` — see
  `tools/tuning-dash/README.md`; `just tune <spec>`, `just tune-dash`, `just tune-test`.
- Sweep spec format (YAML/JSON) and `out/sweeps/<name>/sweep.json` (format 1).
- `tuning-probe` CLI (`run <job.json>`, `resolve`, `diff`, `promote`) and library
  (`tuning_probe::{path, overrides, probe, promote, textedit, diff}`): parameter targets
  `<file>:<field path>` with selectors, in-memory overrides (`Overlay` implements
  `simcli::DataSource`), one-line text edits of JSON/TOML data files.
- `docs/TUNING_CHANGELOG.md` (created by the first promote): one line per promoted file.
- Presets: `tire_lateral_bcde`, `tire_longitudinal_bcde`, `spring_damper`, `arb_balance`,
  `impulse_to_lattice_scale`, `plasticity_scale`, `barrier_speeds`, `compare_sedan_coupe`,
  `damage_scenarios_plasticity`, `sedan_brakes`.

## Needs
| from | what | why | workaround |
|---|---|---|---|
| 03 (B, damage) | `barrier::run_scenario_with(&BarrierScenario, &LoadedCar, &ModeTable)` (or `run_scenario` over a `simcli::DataSource`) | the probe applies overrides in memory; `run_scenario` only reads a data root on disk | `barrier_scenario` points copy cars/tires/lattices/materials/modes to a scratch folder (~ms) |
| 03 (B) | `DamageTuning` fields loadable from data (optional) | solver/buckle constants cannot be swept today | none; sweep `ModeTable`, materials and lattices instead |
| 10 (I, harness/CI) | run `python3 -m unittest discover -s tools/tuning-dash/tests` in CI (needs PyYAML) | the Python side is not covered by `cargo test` | `cargo test --workspace` covers the Rust probe; run `just tune-test` locally |
| 04 (C, testbed) | open a `replay.bin` by URL/path (`?replay=…`) | the dashboard links each scenario point's replay; a click-through would replace thumbnails | top-down path thumbnails from `metrics.csv` |

## Contract change requests
None. Everything goes through existing public APIs (`wp_contracts::parse_*`, `simcli`
`DataSource`/`run`/`load_car`/`load_mode`, `wp_vehicle::metrics`, `wp_damage::barrier`).

| # | Change (type / field / variant) | Why | Who is affected | Status |
|---|---|---|---|---|

## Changes outside my crates
- `Cargo.toml` (root): `tools/tuning-dash/probe` added to workspace members (`Cargo.lock`
  updated; no new external dependencies — serde, serde_json, toml, ron are workspace deps).
- `justfile`: recipes `tune`, `tune-dash`, `tune-test`.
- Nothing reachable from `wp_wasm` (the probe is a leaf tool crate).

## Perf numbers
| Measurement | Machine | Native | Browser | Budget |
|---|---|---|---|---|
| `vehicle_metrics` point (all but top speed) | cloud container, 4 cores | 0.3 s | — | — |
| `barrier` point, 5 speeds | cloud container | 0.1 s | — | — |
| `tire_lateral_bcde`, 40 points, 4 workers | cloud container | 2.7 s | — | — |

## Findings from the presets (for the owners; not changed here)
- **02 vehicle / skidpad metric:** on `debug_sedan` the skidpad `max_lat_g` peaks exactly at
  the file value of lateral B and C (B ×0.9 → 0.886, ×1.0 → 0.891, ×1.1 → 0.853; C ×1.1 →
  0.855) while the understeer gradient moves smoothly. Looks like a measurement artifact
  (speed ramp / "1 s on the line" criterion) rather than tire behaviour; worth a look before
  anyone tunes B/C against skidpad g. Sweep: `tire_lateral_bcde`.
- **02 vehicle / power-on metric:** `debug_coupe` `power_on_curvature_ratio` = 36 (sedan
  0.67); the coupe probably spins in the test, so the ratio is not meaningful there.
  Sweep: `compare_sedan_coupe`.
- **03 damage:** `plasticity_scale` 1.2 and 1.4 give identical crush/health at every speed
  (saturation somewhere); 0.6 breaks the calibration checks (frontal 56 km/h no longer a total
  loss; side door severity below range). Sweeps: `plasticity_scale`,
  `damage_scenarios_plasticity`.
- **03 damage:** `impulse_to_lattice_scale` 1.0–1.2 meets prompt 03's 56 km/h frontal target
  (0.57 / 0.69 m crush, TotalLoss, not at 25 km/h); 0.8 and below stop being a total loss at
  56. Sweep: `impulse_to_lattice_scale`.

## Known gaps
See `tools/tuning-dash/README.md` → Known gaps (thumbnails are paths, not frames;
`DamageTuning` is code; promote does not commit — owners commit data changes with `[bless]`).

## Lead feedback
(Written by the integration lead.)
