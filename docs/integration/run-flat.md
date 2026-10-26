# Run-flat tyres integration notes

- **Workstream:** owner bug "my runflats don't work as intended" (2026-10-26)
- **Branch:** `claude/project-thread-gzll20`
- **Crates touched:** `wp_vehicle` (constant + helper), `wp_sim` (per-vehicle flag), `wp_wasm` (career wiring, HUD/audio readout)
- **Contract version built against:** unchanged (no `wp_contracts` edit)

## What was wrong
The `tires_runflat` mod filled `wp_economy::ToolLoadout::run_flat`, but nothing ever read it:
a spiked, crash-punctured or carried-flat tyre on a car with run-flats deflated to its rim
exactly like a plain tyre. The player paid $1400 and +1.5 kg unsprung per corner for nothing.

## Exposes
- `wp_vehicle::vehicle::RUN_FLAT_MAX_DEFLATION` (0.2) and `run_flat_deflation(p, run_flat)`:
  a run-flat tyre never deflates past 0.2 for the tyre model (grip × 0.9, corner ~6 % of the
  tyre radius lower, rolling resistance × 1.8, never on its rim; `RIM_FROM` is 0.9).
- `wp_sim::Sim::set_run_flat(id, fitted)`: set before the first tick, like `set_tire_condition`.
  Applied after spikes, crash damage and the career tyre condition are merged, so every source
  of puncture is capped. Physics, so identical in Arcade and Simulation.
- `wp_sim::Sim::tire_deflation(id)`: the deflation the handling drives on; `wp_wasm`'s
  `tire_punctures()` (tyre squash in the view) and the audio flap/grind now read it.

## Behaviour kept on purpose
- Spike strips still puncture run-flats (blow-out pop, police `SpikeHit` event). The car limps on.
- After the run a spiked run-flat is still recorded flat on the account and needs a new tyre,
  as real run-flats do; the next run drives on it as a run-flat again.

## Tests
`crates/wp_sim/tests/run_flat.rs`: four flat tyres, 6 s full throttle, both modes. Plain tyres
ride on the rims every tick and reach 4.9 m/s; run-flats never touch the rims and reach 14.3 m/s.

## Known gaps
- Only the career sets the flag (multiplayer and free-roam cars have no mods).
- No tier scaling: tier ≥ 1 is one behaviour (only tier 1 exists in `data/mods/tires.json`).
