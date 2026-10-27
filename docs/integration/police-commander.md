# Police commander integration notes

- **Workstream / prompt:** 06 police, owner request 2026-10-27
- **Branch:** `claude/project-thread-u6pazm`
- **Crates / folders owned:** `crates/wp_police`, `data/police`
- **Contract version built against:** 0.4.2 (no contract change)

## Owner request (2026-10-27)

> Implement a "police commander" AI in the background that tells the environment how many and
> where to deploy cops to catch racers, have him react and get information from dispatch
> (reckless driver calls). The police commander is limited in resources based on driver
> bounty, wanted level, and time of day, with daytime being the most resources.

> Remember that we're supposed to despawn patrol vehicles deemed too far and "no longer in the
> chase".

## Exposes
- `wp_police::commander` (`CommanderConfig`, `Allotment`, `CommanderBudget`, `CommanderView`,
  `CommanderOrder`, `OrderKind`, `CallIntel`).
- `PoliceSystem::commander() -> CommanderView`, `PoliceSystem::commander_budget()`.
- `PoliceConfig::commander` (`data/police/commander.toml`, optional in a tables directory;
  built-in when missing).
- `PoliceStats::commander_orders`, `PoliceStats::far_stand_downs`.
- wasm: `police_commander()` JSON; testbed `sim.policeCommander()`.

## How it plugs in
- `think`: `commander_think` runs after the racer rules (fresh levels) and before the patrol
  pool, dispatch and reinforcements, so every spawn that tick sees the new budget.
- `unit_cap()` is `min(presence caps, budget.ground_units)`; `place_roadblock`, `place_spikes`
  and `request_helo` check `min(table max, budget)`, for the AI and a human captain alike.
  `CaptainView.readiness` shows the effective maxima.
- `on_violation` (report branch) logs every call to the commander.
- `maintain_pool`: table patrols + surge patrols; the surge part only while
  `ground_units_alive < unit_cap()`.
- `release_far_units`: also releases far `Pursue` units, and turns a released pool unit into
  a responder (despawns out of sight; the pool refills its district out of sight).
- Races: `commander_active()` is false while `start_race` has set a race, so race profiles keep
  their own numbers.

## Tests
`cargo test -p wp_police --test commander` (budget by phase / level / bounty, cap respected,
repeat calls by day, night calls need a bounty, district surge, switched off) and the unit tests
in `commander.rs`.

## Contract change requests
None.

## Changes outside my crates
- `crates/wp_wasm/src/lib.rs`: `police_commander()` binding.
- `apps/testbed-web/src/wasm/sim.ts`: `policeCommander()` accessor.

## Known gaps
- No testbed panel shows the commander yet (data is available through `police_commander()`).
- The server (`apps/server` / `wp_net`) does not replicate `CommanderView` to a captain client.
