# wp_police

**Owned by workstream 06 (police & pursuit AI).** Data: `data/police/*.toml`.

Heat/bounty, day/night presence and dispatch, patrol and pursuit AI, police tools (roadblocks,
spike strips, helicopter), racer gadgets (radar detector, scanner) and the captain interface.
Pure Rust, no renderer, builds for `wasm32-unknown-unknown`.

## Design pillars
- Chases are **slow, calculated and lethal to both sides**. Pursuers are speed-capped, plan
  corners conservatively and spend a **risk budget**; a cop that drives like an arcade cop wrecks.
- Police win by **containment and attrition**: shadowing, box-ins, PIT at low speed, rolling
  blocks, spikes/roadblocks ahead of the predicted path, a helicopter keeping visual.
- Racers win by **breaking line of sight** for the escape time, reaching a **safehouse**, or
  disabling pursuers (which raises heat and bounty).
- **Day vs night is tables, not code**: every phase-dependent number is a `[x.dawn/day/dusk/night]`
  table in `data/police/`.

## How it works (one tick)
`PoliceSystem::step(world, racers, events, dt, out)` is called once per physics tick by `wp_sim`.

1. **Racers** are synced (position/velocity); spike strip crossings are checked per wheel.
2. **Events**: `TrafficViolation` adds heat (full if a unit/camera *witnessed* it, 20% otherwise
   as a civilian *report*); `TotalLoss` of a wanted racer is a capture; `Contact` between a unit
   and a racer marks the unit, civilian–civilian contacts above a speed create crash incidents.
3. **Driving** (every tick): internally driven units integrate with the built-in kinematic driver.
4. **Think** (`think_hz`, default 10 Hz): line of sight → rules (heat decay, escape, capture,
   safehouse) → health check (disengage/pull over/wreck) → patrol pool → incidents →
   reinforcements → helicopter → AI captain tools → tactics and drive commands.

### Heat & wanted levels (`heat.rs`)
- Heat 0–100, levels L1..L5 at `level_thresholds`.
- Decays only when unseen and not pursued, after `decay_grace_s`, at
  `decay_per_s[phase] × decay_district_scale[district]` (faster at night, Suburb, Industrial).
- While pursued and seen: `pursuit_heat_per_s` (long chases escalate). Disabling a cop: `cop_disabled_heat`.
- **Pursuit start**: a wanted racer kept within `recognition_range_m[phase]` with line of sight
  for `recognition_time_s[phase]` (a glimpse at night is not enough).
- **Escape**: no unit, camera or helicopter has line of sight for
  `escape.base_s[level] × escape.phase_scale[phase]`.
- **Safehouse**: inside a safehouse POI for `dwell_s` with no unit within `clear_radius_m`;
  heat is multiplied by `heat_scale_on_enter`.
- **Capture**: speed < `stop_speed_mps` for `stop_time_s` with a unit within `unit_radius_m`, or
  `TotalLoss` while wanted. Heat resets to 0.
- **Bounty** = `per_heat_second × ∫heat/100 dt + per_race_won_wanted × races + per_cop_disabled × cops`.
  `BountyRecord` is persisted by prompt 08 (`bounty_record` / `set_bounty_record`). It sets
  reinforcement priority (up to `priority_max_reduction` faster response). Night stakes: `payout_scale()`.

### Presence & dispatch (`dispatch.rs`)
- Pool units per district per phase from `presence.toml [patrols.*]`. Shortfalls are spawned;
  surplus idle units retire when no racer is within 300 m.
- **Incidents**: civilian reports (dispatched after `report_dispatch_delay_s[phase]`) and crash
  scenes (`report_crash_scene`, or automatically from contacts), which hold a unit for `crash_scene_s[phase]`.
- **Reinforcements**: each pursued racer wants `units_per_level[level]` ground units. A deficit
  creates a request due after `response_s[phase]` (Day 20–40 s, Night 60–150 s). When it comes due,
  an idle unit within 700 m is committed, or a new unit spawns ≥ `spawn_min_distance_m` away, out of sight.
- **Helicopter**: `count`, `launch_s`, `unavailable_chance` per phase. It requests from `min_level`.
  It is kinematic, gives line of sight to every unit, and is **not** a weapon.
- Every assignment change emits `PoliceEvent::Radio`.

Assignments: `Patrol`, `Respond(incident)`, `Pursue(target)`, `Intercept(point)`, `RoadblockAt`,
`SpikeAt`, `HeloTrack`, `OutOfService`.

### Pursuit (`pursuit.rs`, `unit.rs`)
- Pursuers follow the racer's **breadcrumb trail**, which is recorded only while the racer is
  seen, extended by its predicted path. Losing sight means losing the scent.
- Speed = `min(max(target × (1 + target_speed_margin), 10), pursuit_cap_mps[phase])`. Corners are
  planned at `pursuit_cornering_g`, or `shadow_cornering_g` once the budget is spent.
- **Risk budget**: time above `risk.lateral_g`, near misses and collisions cost budget. A unit
  with no budget only shadows, at ≤ 1.3× the limit.
- **Tactics**, picked closest pursuer first:
  - `BoxIn`: racer < `boxin_max_kmh` and ≥ 2 budgeted units within 60 m; rear, front-left and front-right slots.
  - `Pit`: racer < `pit_max_kmh[mode]` (55 Sim / 70 Arcade), ≥ `pit_min_straight_m` of straight
    road ahead, unit has budget and is off cooldown.
  - `RollingBlock`: a unit ahead of the racer slows in front of it.
  - `Shadow`: hold 25–40 m back, 12 m more per extra unit; close to 8 m when the racer stops.
  - `Intercept`: units off the trail or far behind take the lane-legal fastest route to the
    racer's predicted position 10–20 s out.
- **Disengage**: health < `disengage_health`, a bent wheel (`|toe| > bent_wheel_toe_rad`), a flat
  or a lost wheel, or a dead engine → pull over. Total loss at `total_loss_health[mode]`.

The **built-in driver** (`step_ground`) is a point mass that keeps right
(`LANE_OFFSET_M`), uses pure-pursuit steering, saturates at a grip limit, crashes when it runs
wide and has a background traffic-hazard model. It stands in until physical police cars exist;
`drive_input` converts a `DriveCommand` into `VehicleInput` for physical units.

### Tools & gadgets (`tools.rs`)
- `plan_site` puts a tool on the predicted path between `min_ahead_m` and `max_ahead_m`, never
  within 15 m of an intersection, and with approach sight distance ≥ `min_sight_m` (120 m).
  Sight distance is the minimum over **every** legal approach, so a site just after a turn-in
  point is a blind corner.
- **Roadblock**: 2–3 parked units plus barrier `StaticCollider`s with mass. Never on a highway
  when the level is above `highway_max_level`.
- **Spike strip**: a line collider. Wheels that cross it deflate linearly over `deflate_s` (5 s)
  → `tire_puncture(entity)` for `HandlingEffects::tire_puncture`.
- **Radar detector** `radar(pos, tier)`: units in Patrol/Pursue state within `radar_range_m[tier]`.
- **Scanner** `scanner(tier, since_s)`: radio lines delayed by `scanner_delay_s[tier]` (10/6/3 s).

### Captain (`captain.rs`)
`CaptainCommand` (assign unit, roadblock, spikes, helicopter, priority target, call off) is
validated against unlocks (`presence.toml [unlocks]`), cooldowns, budgets and the placement
rules, and returns `CaptainError` on refusal. Use `to_bytes`/`from_bytes` for the payload of
`ClientToServer::CaptainCommand`. `captain_view()` returns units, incidents, heat by district,
racers' last-known positions with staleness, tools and readiness. Without a human captain
(`set_human_captain(false)`), the dispatcher places tools and requests the helicopter itself.
Player chase units get `unit_order()` **suggestions** only.

## Tuning tables (`data/police/`)
| File | Contents |
|---|---|
| `heat.toml` | Level thresholds; violation weights; witness, recognition, decay and escape tables per phase; capture, safehouse and bounty rules |
| `presence.toml` | Patrols per district × phase, reinforcement response times, report delays, helicopter availability, tool/tactic unlock levels, crash scenes |
| `pursuit.toml` | Think rate, speed factors and caps, cornering g, LOS range, unit dynamics and crash severity per mode, risk budget, traffic hazards, tactic parameters |
| `tools.toml` | Roadblock, spike strip and helicopter parameters, radar/scanner tiers, captain hold time |

The tables are compiled in (`PoliceConfig::builtin()`) and can be loaded from disk with
`PoliceConfig::load(dir)`. A test checks that both copies match.

## Measured behaviour (`apps/simcli/scenarios/police`, procedural city)
| Scenario | Result |
|---|---|
| Patrol presence (4 phases) | Exact table counts after warm-up |
| Day speeder, Core, 2× limit (5 runs) | Pursuit at 4.5 s mean (≤ 20 s every run); reinforcement 30 s |
| Night speeder, 03:00 (8 runs) | Pursuit at 171 s mean (3 of 8 runs < 60 s: the racer passed a cop); reinforcement 110 s |
| Cop self-wreck, Simulation (20 pursuits) | 4.1% of engaged units; a reckless config (no budget, high caps) does measurably worse |
| Tool placement (5 runs, L4) | 40 tools, minimum sight 120.1 m |
| Escape / capture / safehouse / total loss | Each rule fires in its scripted case |

## Perf
`cargo run --release -p wp_police --example police_bench`: 13 patrols + reinforcements, 4 wanted
racers → **0.030 ms per 60 Hz frame** (native, release, cloud Xeon 2.1 GHz). Budget 0.5 ms
native / 1.0 ms browser. Browser not measured yet.

## Tests
```sh
cargo test -p wp_police                          # unit + system tests
cargo test -p simcli --test police_scenarios     # scenarios, ratios, determinism
just sim apps/simcli/scenarios/police/day_core_speeder.ron
cargo run --release -p wp_police --example police_scenario -- <file.ron> [--tables <dir>]
```

## Known gaps
- Own `RoadGraph` and `procedural_city` stand in for `wp_world`'s network and LOS queries.
- The built-in kinematic driver and its traffic-hazard model stand in for physical cars and real traffic.
- `PoliceSystem::new()` (used by `wp_sim` today) has no road network, so it tracks heat but spawns no units.
- Scripted racers don't react to PIT or box-ins; physics resolves those in the real game.
- Captain-command events are emitted on the next `step`.
- No JSON Schema for `data/police/*.toml`; validation lives in `PoliceConfig::validate` plus serde `deny_unknown_fields`.
