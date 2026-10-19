# Police integration notes

- **Workstream / prompt:** 06 — police & pursuit AI
- **Branch:** `ws/police`
- **Crates / folders owned:** `crates/wp_police`, `data/police/`, `apps/simcli/scenarios/police/`
- **Contract version built against:** 0.1.0

## Exposes
- `PoliceSystem::step(world: &dyn WorldQuery, racers: &[RacerView], events: &[SimEvent], dt, out)`.
  The signature is unchanged from the stub, so `wp_sim` compiles as-is.
- Construction: `PoliceSystem::new()` (built-in tables, no road network: heat only) and
  `PoliceSystem::with_city(PoliceConfig, RoadGraph, PoliceSetup { seed, mode, internal_driving, hazards })`.
- Queries: `wanted_level`, `racer`, `units`, `incidents`, `pool_count`, `tire_puncture(entity) -> [f32; 4]`,
  `static_colliders()`, `spike_strips()`, `roadblocks()`, `helicopters()`, `state_hash()`, `stats()`.
- Bounty (for prompt 08): `bounty`, `bounty_record`, `set_bounty_record`, `record_race_won`, `payout_scale`.
- Physical units (for prompt 12): the `UnitSpawned` / `UnitDespawned` events, then
  `bind_unit_entity(unit, entity)`, `unit_input(unit) -> VehicleInput` and
  `sync_unit(unit, UnitSync { pos, vel, heading, health, effects })`. Units whose effects show a bent wheel pull over.
- Player chase units (for prompt 07): `add_player_unit(player, entity, pos, heading)` and `unit_order(unit)` (suggestion only).
- Captain API (for prompt 07): `captain_command(CaptainCommand) -> Result<(), CaptainError>`,
  `CaptainCommand::{to_bytes, from_bytes}` (JSON; the payload of `ClientToServer::CaptainCommand`),
  `captain_view() -> CaptainView`, `set_human_captain(bool)`.
- Racer gadgets (for prompts 07/08 to gate by tier): `radar(pos, tier) -> Vec<RadarBlip>`,
  `scanner(tier, since_s) -> Vec<RadioLine>`, `radio_log()`.
- Crash scenes (for prompt 05): `report_crash_scene(pos)`. Civilian–civilian `SimEvent::Contact`s also create crash scenes automatically.
- `SimEvent::PoliceEvent { kind, payload }`: `kind` = `wp_police::events::kind::*`, `payload` =
  the JSON of `wp_police::PoliceEvent`. Decode with `PoliceEvent::from_sim_event`. Variants:
  Radio, WantedLevelChanged, PursuitStarted, PursuitEnded, UnitSpawned, UnitDespawned, UnitDisabled,
  RoadblockPlaced, SpikesPlaced, ToolRemoved, SpikeHit, HeloLaunched, PitAttempt.
- Data: `data/police/{heat,presence,pursuit,tools}.toml` (see the crate README for meanings).
- Headless scenarios: `wp_police::scenario` plus `apps/simcli/scenarios/police/*.ron`.

## Needs
| From | What | Workaround now |
|---|---|---|
| `wp_world` (05) | Road graph (nodes, segments, limits, one-way, class, district), POIs (safehouses, cameras), building occluders, or a converter into `wp_police::RoadGraph` | Own `RoadGraph` plus `procedural_city()` |
| `wp_world` (05) | Line-of-sight query `line_of_sight(a, b) -> bool` (ideally on `WorldQuery`) | `RoadGraph::line_of_sight` against AABB occluders |
| `wp_world` (05) | `TrafficViolation` events from real detection | Scenarios generate speeding events |
| `wp_vehicle` / 05 | The shared AI driving controller (`DriveCommand` → `VehicleInput`) | `unit::drive_input` (pure pursuit) and the `step_ground` kinematic driver |
| `wp_sim` (12) | Spawn vehicles for units, call `bind_unit_entity`, `unit_input` and `sync_unit`; merge `tire_puncture` into `HandlingEffects`; insert `static_colliders`; build `with_city` from the world's graph | Units are driven internally |
| `wp_net` (07) | Player roles (captain / chase seats) → `set_human_captain`, `add_player_unit`; replicate `CaptainView` | None |

## Contract change requests
| # | Change | Why | Who is affected | Status |
|---|---|---|---|---|
| 1 | `HandlingEffects.tire_puncture: [f32; 4]` | Spike strips | 02, 03, 12 | **Already in v0.1.0**, nothing to do |
| 2 | Define the `SimEvent::PoliceEvent` payload: keep the opaque variant; payload = JSON of `wp_police::PoliceEvent`, kinds in `wp_police::events::kind` | Prompt 06 deliverable | 07 (replication), 08 (payouts on capture/escape), 04 (UI) | Proposed; no contract edit needed. 07 may switch the codec to binary |
| 3 | `ClientToServer::CaptainCommand(Vec<u8>)` payload = JSON of `wp_police::CaptainCommand` | Captain client | 07 | Proposed; no contract edit needed |
| 4 | `WorldQuery::line_of_sight(a: [f32; 3], b: [f32; 3]) -> bool` | Police LOS on the real city | 05, 12 | Requested |
| 5 | Promote `RoadGraph` (or an equivalent read-only road-network trait) into contracts | So police, traffic and the minimap share one network | 05, 06 | Requested; 05 to propose a shape |
| 6 | JSON Schema for `data/police/*.toml` | Data validation parity with other data | Lead | Optional; serde `deny_unknown_fields` plus `PoliceConfig::validate` cover it today |

## Changes outside my crates
- **Root `Cargo.toml`**: `[profile.dev.package.wp_police] opt-level = 2`. Without it the debug
  scenario tests take over 90 s; with it, about 11 s.
- **`apps/simcli/Cargo.toml`**: depends on `wp_police`.
- **`apps/simcli/src/main.rs`**: files under a `police/` directory go to `wp_police::scenario`.
- **`apps/simcli/tests/scenarios.rs`**: skips `scenarios/police/` (different format).
- **`apps/simcli/tests/police_scenarios.rs`**: new; scenario expectations, determinism, night/day ratios, risk-budget check.

## Perf numbers
| Measurement | Machine | Native | Browser | Budget |
|---|---|---|---|---|
| `PoliceSystem::step` per 60 Hz frame, 13 patrols + reinforcements, 4 wanted racers (`police_bench`) | Cloud Xeon 2.1 GHz, release | 0.030 ms | not measured | 0.5 ms / 1.0 ms |

## Known gaps
- Road graph, LOS, the driver and the traffic-hazard model are stand-ins (see Needs).
- `wp_sim` still uses `PoliceSystem::new()`, so the browser testbed has no police units until 12 wires `with_city`.
- Scripted racers don't react to PIT or box-ins; physics resolves those.
- Night pursuit start varies: 3 of 8 runs under 60 s. The mean is 171 s, and the test asserts the mean and the ratio.

## Lead feedback
