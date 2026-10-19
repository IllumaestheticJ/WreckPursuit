# PROMPT 20 — Physics/damage tuning dashboard (`tools/tuning-dash`)

Read `CLAUDE.md`, `docs/INTEGRATION.md`, `apps/simcli/SCENARIOS.md` (if prompt 10 has landed; otherwise its prompt `prompts/10_test_harness_ci.md` describes the output files). You own `tools/tuning-dash` (Python or TS, your call) . Branch `ws/tuning-dash`.

## Goal
Make human tuning sessions fast: run a parameter sweep over any `VehicleDef`/`TireDef`/`ModeTable`/`LatticeDef` field(s) through `apps/simcli`, and show the results as comparable plots and tables, plus side-by-side comparison of two cars or two modes.

## Work
- Sweep runner: YAML sweep spec (scenario, parameter paths, ranges/lists, parallel workers) → runs `simcli` with overridden data into `out/sweeps/<name>/`.
- Dashboard (local web page): metrics vs parameter plots (0–100, skidpad g, understeer gradient, yaw overshoot, crush depth vs speed, components failed, repair cost), replay thumbnails if available, pass/fail against scenario expectations, diff view between two data files, and a "promote" button that writes the chosen values back into the data file with a changelog line.
- Presets: the standard sweeps from prompts 02/03 (tire B/C/D/E, spring/damper, `impulse_to_lattice_scale`, `plasticity_scale`, barrier speeds).
- Follow the `dataviz` skill if available for chart styling.

## Deliver
Tool, README, `docs/TUNING.md` (how a tuning session runs, from sweep to promoted values).
