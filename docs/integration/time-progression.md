# Time progression (day/night shifts) integration notes

- **Workstream / prompt:** career time progression (owner request 2026-10-22)
- **Branch:** `claude/project-thread-v6wqdx`
- **Crates / folders owned:** `crates/wp_world/src/progression.rs`
- **Contract version built against:** 0.3.0

## What it does
The career day is two playable shifts, each with a budget of in-game minutes:

| Shift | Clock window | Budget | Purpose |
|---|---|---|---|
| `Shift::Day` | 08:00–18:00 | 600 min | free roam, sanctioned events, shopping |
| `Shift::Night` | 20:00–04:00 | 480 min | street racing, meets |

Driving spends budget continuously (`drive_ratio` = 10 in-game s per simulated s, so a day is
one real hour of driving). Activities spend it in lumps. The environment clock follows the
budget: half the budget used = the sun halfway through the window. When the budget runs out a
20 s dusk (18→20) or dawn (04→08) transition plays and the next shift starts. Time spent past
the end of a shift carries over as debt; debt covering a whole shift skips it (an overnight
repair means no racing that night).

## Exposes
All in `wp_world::progression` (re-exported at the crate root: `Activity`, `Shift`,
`TimeEvent`, `TimeProgression`, `TimeRules`).

**Time-cost API (for the garage / economy / race threads)**
- `TimeProgression::spend(Activity, &mut Vec<TimeEvent>) -> SpendReceipt { minutes, ends_shift }`
- `TimeProgression::cost_minutes(Activity) -> u32` and `fits(Activity) -> bool` for UI previews
  ("Repair takes 1 h 30 min — night will fall before it's done").
- `TimeProgression::spend_minutes(u32, &mut events) -> bool` for anything without an `Activity`.
- `Activity::{GarageVisit, Repair { labour_hours }, ModInstall { labour_hours }, RaceEntry,
  ImpoundRetrieval, Custom { minutes }}`. For repairs pass the summed `RepairLine::labour_hours`
  of the lines being fixed; minutes = `ceil(hours × 45)`, at least 15 (`ActivityCosts`).

**Driving the world**
- Every tick the player is driving: `time.step(PHYSICS_DT, &mut events)` then
  `time.apply(&mut world.clock)` after `World::step` (overrides the clock's ratio advance).
  Do not step while in menus / the garage; those cost time through `spend`.
- `time_of_day()`, `phase()`, `remaining_min()`, `fraction_used()`, `carried_min()`, `day()`,
  `shift()`, `is_transitioning()` for the HUD.
- `end_shift()` ("skip to night / sleep"), `finish_transition()` (loading screen skip).
- `TimeEvent::{LowTime, ShiftEnded, ShiftSkipped, ShiftStarted}` — end the free-roam session or
  race meet on `ShiftEnded`; re-roll weather, density and the race calendar on `ShiftStarted`.
- `TimeProgression` is `Serialize + Deserialize + PartialEq`: save it with the career profile.
- `TimeRules` (serde, `deny_unknown_fields`) holds every tunable; `TimeRules::validate()`.

## Needs
- **wp_sim / wp_wasm (lead, testbed):** own a `TimeProgression` next to the career session, step
  it while driving and call `apply` after the world step. Not wired in this PR, because writing
  the clock from outside the sim would need a replay op to stay replay-safe in the testbed.
- **Garage thread (`claude/project-thread-thv5w5`):** call `spend(Activity::Repair { .. })` /
  `spend(Activity::GarageVisit)` when a repair is confirmed; show `cost_minutes` in the quote.
- **Race thread:** `spend(Activity::RaceEntry)` on joining; pick night meets only when
  `shift() == Shift::Night`.

## Contract change requests
None. Uses `wp_contracts::{TimeOfDay, DayPhase}` as they are.

## Changes outside my crates
None.

## Perf numbers
Constant-time per call (a few float ops per tick); not measurable against the budgets.

## Known gaps
- Rules are code defaults; no `data/` file yet (would need a schema).
- Weather does not change at shift boundaries by itself; react to `ShiftStarted`.
- Not wired into the browser testbed yet (see Needs).
