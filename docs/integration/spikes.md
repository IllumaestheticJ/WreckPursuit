# Spike strips always manned — integration notes

- **Workstream:** spike strip with no police car (playtest bug), PR #54 (`ws/spikes`)
- **Ported onto:** `lead/round12` by the PR-ports helper (`lead/pr-ports`), from 281187e4
- **Crates / folders:** `wp_police` (dispatch, tools), plus a small hook in `wp_sim::police_link`
- **Contract version built against:** 0.3.1 (no contract change)

## What changed
A spike strip now exists only while a police unit is parked beside it:

- A captain / AI request lays the strip **and spawns its car at once**, on the right shoulder of
  the racer's carriageway (row 133 layout unchanged). round12's row 131 made spike cars wait
  (`PendingToolCar`) until their spot was out of every racer's view while the strip already lay
  on the road — the "strip with no police car" of the playtest report. Holding the strip back
  with its car was tried first and drops spike hits to zero (`police_scenario`
  `tool_placement` 10 → 0, `radio_city_chase` 16 → 0: the spot of a site with ≥ 120 m approach
  sight is almost always in view until the racer has passed), so spike cars are now **exempt
  from row 131's wait**: they appear with the strip, ≥ `min_ahead_m` (150 m) ahead. Roadblock
  cars still wait as before.
- The site must have a shoulder spot the car can appear on (row 75: `spot_clear`, clear of
  parked cars, cones and vehicles): `tools::plan_site_where` takes the first rule-valid site
  that passes; none → `SiteError::NoSite`, retried on a later think.
- `place_spikes_at` (scenario / testbed placement) spawns a spike unit too; before, it laid a
  strip with no car at all.
- A strip is lifted (`ToolRemoved`) the moment its unit leaves `SpikeAt` (wrecked, pulled
  over, reassigned, e.g. to a row 84 search) or is despawned, and at think rate when the unit
  is more than `dispatch::SPIKE_UNIT_LEASH_M` (15 m) from where it parked (`SpikeStrip::post`;
  a row 75 upright reset that moves it further lifts the strip too).
- `wp_sim` withdraws a spike unit it could not give a physical car (vehicle cap of
  `MAX_VEHICLES - POLICE_SLOT_RESERVE` reached, or the spawn failed), strip and all.
- **Scripted race-profile strips** (`data/police/races.toml`, `SpikeStrip::scripted`) are
  **exempt**: they have no unit, and no profile stakes one out beside its strip (the only one,
  `grid_highway_sprint` at 0.85, has stakeouts at 0.3 / 0.55). The rule applies to a scripted
  strip only if it is ever given a unit.
- Spikes still unlock at heat level 4 (row 156); unchanged.

Effect in the police scenarios (5 runs each): spike hits `tool_placement` 10 → 8,
`radio_city_chase` 16 → 14 (some strips are now lifted when their car is knocked away).

## Exposes
- `PoliceSystem::withdraw_unbodied_spike_unit(unit) -> bool`
- `wp_police::dispatch::SPIKE_UNIT_LEASH_M`
- `SpikeStrip::post: Option<[f32; 2]>` (serde default)
- `wp_police::tools::plan_site_where`

## Changes outside my crates
- `crates/wp_sim/src/police_link.rs` (`reconcile_police_units`): after the body spawn loop, call
  `withdraw_unbodied_spike_unit` for units still without a car.

## Tests
- `tests/system.rs`: `spike_strip_never_lies_without_its_unit`,
  `spike_strip_lifted_when_its_unit_leaves_the_post`,
  `unbodied_spike_unit_is_withdrawn_with_its_strip` (from the PR), and the captain-tools test
  now checks a requested strip has its car beside it and no car waiting.
- `race.rs` `spikes_cover_part_of_the_course_only`: a scripted strip without a unit survives
  the tool sweep.

## Goldens
- `golden_spike_strip` re-blessed: the scenario's strip now has its unit, so the state hash
  changes from tick 24 (same tick as the PR on `main`). Its expectations (spiked wheels, speed
  ranges) are unchanged and pass. No other golden changed.

## Known gaps / open questions
- Row 131 exception for spike cars (above) — owner call: accept a spike car appearing at
  ≥ 150 m, or make spike cars drive in from a hidden spot (bigger change).
- The spike car's spawn is not in the row 131 pop-in audit (`record_spawn` needs the world;
  `place_spikes` has none).
- Roadblocks have the same shape (barriers can outlive their units); not changed here.
