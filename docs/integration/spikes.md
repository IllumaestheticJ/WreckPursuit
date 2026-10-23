# Spike strips always manned — integration notes

- **Workstream:** spike strip with no police car (playtest bug)
- **Branch:** `ws/spikes`
- **Crates / folders owned:** `wp_police` (dispatch), plus a small hook in `wp_sim::police_link`
- **Contract version built against:** 0.4.0 (no contract change)

## What changed
A spike strip now exists only while a police unit is parked beside it:

- `place_spikes_at` (scenario / testbed placement) spawns a spike unit too; before, it laid a
  strip with no car at all.
- A strip is lifted (`ToolRemoved`) the moment its unit leaves `SpikeAt` (wrecked, pulled over,
  released) or is despawned, and at think rate when the unit is more than
  `dispatch::SPIKE_UNIT_LEASH_M` (15 m) from its post.
- `wp_sim` withdraws a spike unit it could not give a physical car (vehicle cap of
  `MAX_VEHICLES - POLICE_SLOT_RESERVE` reached, or the spawn failed), strip and all. This was the
  likely cause of the playtest report: a full city left the spike unit kinematic and invisible.

## Exposes
- `PoliceSystem::withdraw_unbodied_spike_unit(unit) -> bool`
- `wp_police::dispatch::SPIKE_UNIT_LEASH_M`

## Changes outside my crates
- `crates/wp_sim/src/police_link.rs` (`reconcile_police_units`): after the body spawn loop, call
  `withdraw_unbodied_spike_unit` for units still without a car.

## Goldens
- `golden_spike_strip` re-blessed: the scenario's strip now has its unit, so the state hash
  changes from tick 24. Its expectations (4 spiked wheels, speed ranges) are unchanged and pass.

## Known gaps
- Roadblocks have the same shape (barriers can outlive their units); not changed here.
