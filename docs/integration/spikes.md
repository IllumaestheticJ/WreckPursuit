# Spike strips always manned — integration notes

- **Workstream:** spike strip with no police car (playtest bug), PR #54 (`ws/spikes`)
- **Ported onto:** `lead/round12` by the PR-ports helper (`lead/pr-ports`), from 281187e4
- **Crates / folders:** `wp_police` (dispatch, tools), plus a small hook in `wp_sim::police_link`
- **Contract version built against:** 0.3.1 (no contract change)

## What changed
A spike strip lies on the road with its police unit beside it, or, when that car could only
appear in a racer's view, unmanned (rows 131 / 158: no police car appears in view; lead,
2026-10-23):

- A captain / AI request lays the strip at once and queues its car (`PendingToolCar`) on the
  right shoulder of the racer's carriageway (row 133 layout unchanged). On the next think
  (`maintain_tool_cars`, which has the world for the view check) the car gets one chance: spot
  hidden, it appears beside the strip (`SpikeStrip::unit` / `post` set, counted in the row 131
  pop-in audit via `record_spawn`); in view, no car is spawned and the strip stays unmanned.
- An unmanned dynamic strip is lifted once its racer is more than `SPIKE_PASSED_M` (25 m) past
  it along the road, when the racer's pursuit ends, or when it expires.
- A manned strip is lifted (`ToolRemoved`) the moment its unit leaves `SpikeAt` (wrecked, pulled
  over, reassigned, e.g. to a row 84 search) or is despawned, and at think rate when the unit
  is more than `dispatch::SPIKE_UNIT_LEASH_M` (15 m) from where it parked (`SpikeStrip::post`;
  a row 75 upright reset that moves it further lifts the strip too).
- The site must have a shoulder spot a car may appear on (row 75: `spot_clear`, clear of parked
  cars, cones and vehicles): `tools::plan_site_where` takes the first rule-valid site that
  passes; none: `SiteError::NoSite`, retried on a later think.
- `place_spikes_at` (simcli scenarios / tests only) spawns its spike unit directly, as in the PR.
- `wp_sim` withdraws a spike unit it could not give a physical car (vehicle cap of
  `MAX_VEHICLES - POLICE_SLOT_RESERVE` reached, or the spawn failed), strip and all.
- **Scripted race-profile strips** (`data/police/races.toml`, `SpikeStrip::scripted`) are
  **exempt** (E agrees): course features with no unit, expiring with `tool_lifetime_s`.
- Spikes still unlock at heat level 4 (row 156); unchanged.

History: the first port made spike cars appear with the strip even in view (holding the
strip back until the car's spot was hidden gave zero spike hits). Replaced by the unmanned
fallback above at the lead's request (owner row 158: no cop may appear in view).

Effect in the police scenarios (5 runs each, `police_scenario`): spike hits `tool_placement`
10 (round12) -> 9, `radio_city_chase` 16 -> 17.

## Exposes
- `PoliceSystem::withdraw_unbodied_spike_unit(unit) -> bool`
- `wp_police::dispatch::SPIKE_UNIT_LEASH_M`, `SPIKE_PASSED_M`
- `SpikeStrip::post: Option<[f32; 2]>` (serde default)
- `wp_police::tools::plan_site_where`

## Changes outside my crates
- `crates/wp_sim/src/police_link.rs` (`reconcile_police_units`): after the body spawn loop, call
  `withdraw_unbodied_spike_unit` for units still without a car.

## Tests
- `tests/system.rs`: `spike_strip_never_lies_without_its_unit`,
  `spike_strip_lifted_when_its_unit_leaves_the_post`,
  `unbodied_spike_unit_is_withdrawn_with_its_strip` (from the PR); the captain-tools test
  checks a requested strip is down at once, its car decided on the next think (beside it, or
  none and no unit on it).
- `race.rs` `unmanned_strip_lifted_once_its_racer_passes`.
- `tests/divided_tools.rs`: spike car checks only when the strip has a car.
- `race.rs` `spikes_cover_part_of_the_course_only`: a scripted strip without a unit survives
  the tool sweep.

## Goldens
- `golden_spike_strip` re-blessed: the scenario's strip now has its unit, so the state hash
  changes from tick 24 (same tick as the PR on `main`). Its expectations (spiked wheels, speed
  ranges) are unchanged and pass. No other golden changed.

## Known gaps
- An unmanned strip is a strip with no police car beside it, by design (see above).
- Roadblocks have the same shape (barriers can outlive their units); not changed here.
