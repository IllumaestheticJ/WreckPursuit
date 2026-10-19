# PROMPT 06 — Police: heat/bounty, dispatch, day/night presence, pursuit AI, captain tools (`wp_police`)

Read `CLAUDE.md`, `docs/INTEGRATION.md`, `docs/CONTRACT_CHANGELOG.md`. You own `wp_police` and `data/police/`. You may read `wp_contracts`, `wp_math`, `wp_world` (road graph, routing, time of day), `wp_vehicle` (AI driving controller used by traffic — reuse it). Branch `ws/police`.

## Design pillars (do not drift from these)
- Chases are **slow, calculated and lethal to both sides**. Nobody is invincible. A patrol car that drives like an NFS cop wrecks itself in this damage model and is *supposed to*.
- Police win by **containment and attrition**, not by speed: shadowing, boxing in, rolling roadblocks, spikes ahead of the predicted path, helicopter maintaining visual so ground units don't have to take risks.
- Racers win by **breaking contact** (line of sight + time), reaching a safehouse, or disabling pursuers — at the cost of heat and repair bills.
- **Daytime**: dense patrols, fast reinforcement, full toolset. **Night**: sparse patrols, slow reinforcements, helicopter availability limited, but bounties and payouts are higher. These are tables, not code branches.

## Work
**Heat & bounty model** (`heat.rs`): per racer `heat: f32` (0–100) with five `WantedLevel`s; sources are `TrafficViolation` events weighted by severity and by *witnesses* (a violation seen by a patrol or camera-POI counts fully; unseen counts at 20% as "reports"); decay when unseen (faster at night, faster in Suburb/Industrial); **bounty** = f(heat history, races won while wanted, cops disabled) persists across sessions (prompt 08 stores it) and determines reinforcement priority and night-race stakes. Capture, escape and safehouse rules: pursuit ends when no unit has LOS for `escape_time(level, phase)` or the racer is inside a safehouse POI for 10 s with no unit within 150 m. Capture when stopped (< 2 m/s for 4 s) with a unit within 15 m, or `TotalLoss` while wanted.

**Presence & dispatch** (`dispatch.rs`): `data/police/presence.toml` — patrol unit count per district as a function of `DayPhase` (e.g. Core: Day 8 / Dusk 6 / Night 3 / Dawn 2; Suburb: 3/2/1/1), reinforcement response time curves (Day 20–40 s, Night 60–150 s), helicopter availability (Day: 1 helo, 45 s launch; Night: 1 helo, 120 s launch, 25% chance unavailable), spike/roadblock unlock by wanted level. Dispatcher assigns units from a pool to: `Patrol(route)`, `Respond(incident)`, `Pursue(target)`, `Intercept(predicted_point)`, `RoadblockAt(segment)`, `SpikeAt(segment)`, `HeloTrack(target)`. Incidents are generated from violations and from crashes (a civilian pile-up draws a unit for N minutes — prompt 05's dynamic crash scenes). Emits radio-log lines (`PoliceEvent::Radio{unit, text}`) for every assignment — the scanner audio and captain UI consume these.

**Patrol & pursuit driving** (`pursuit.rs`, uses the shared AI driving controller): patrol speed = limit; pursuit speed cap = `min(target_speed + 10%, pursuit_cap(phase))` with a **risk budget** per unit (time at > 0.7 g lateral, near-misses, collisions) that, once spent, drops the unit to shadowing mode. Tactics as behaviours: `Shadow` (hold 25–40 m behind, mirror lane), `BoxIn` (2–3 units: front-left, front-right, rear; only below 60 km/h), `PIT` (only below 55 km/h in Simulation, 70 in Arcade, only on straights, only by units with risk budget), `RollingBlock`, `Intercept` (route via road graph to the racer's predicted position 10–20 s out using lane-legal shortest path; this is how slow cops still matter), `Disengage` when damaged below 40% health. Pursuers honour their own `HandlingEffects`; a cop with a bent wheel pulls over.

**Tools** (`tools.rs`): `Roadblock` = 2–3 parked units + barriers as static colliders with mass, spawned ≥ 300 m ahead on the predicted path, never around a blind corner (check sight distance on the road graph), never on highways above a wanted level threshold; `SpikeStrip` = line collider that, on contact, flags `tire_puncture[i]` → grip/radius decay over 5 s (request a `HandlingEffects.tire_puncture:[f32;4]` contract field); `Helicopter` = kinematic entity following the target with a spotlight cone, provides LOS to all units, is **not** a weapon. Racer-side `RadarDetector` (alerts on units within range that are in Patrol/Pursue state) and `Scanner` (surfaces radio-log lines, delayed 3–10 s by tier) are defined here as queries so the UI/economy can gate them.

**Captain interface** (`captain.rs`): a `CaptainCommand` enum (assign unit, request roadblock/spikes/helo, set priority target, call off) with validation against budgets/cooldowns, and a `CaptainView` struct (unit list + states, incidents, heat map by district, last-known racer positions with staleness). Player chase units receive `UnitOrder`s from the captain as suggestions, not forced control. If no human captain exists, the dispatcher acts as captain. This is the entire server-side model prompt 07 replicates to the captain client.

**Day vs night in one place**: every tuning number above lives in `data/police/*.toml` keyed by `DayPhase`; nothing is hard-coded.

## Tests (headless, `apps/simcli/scenarios/police/*.ron` on the procedural city)
- Patrol counts per district per phase match the table after warm-up.
- A racer doing 2× the limit through Core at noon triggers pursuit within 20 s; same at 03:00 takes > 60 s and reinforcements arrive slower (assert ratios, not exact values).
- Cop self-wreck rate: over 20 scripted pursuits in Simulation, < 10% of units total-loss themselves without racer contact.
- Spike/roadblock placement never on a segment with < 120 m sight distance.
- Escape rule and capture rule fire correctly in scripted cases. Determinism hash.

## Deliver
`wp_police`, data tables, tests, README (behaviour write-up + the tuning table meanings), `docs/integration/police.md` (exposes: `PoliceSystem::step(world, racers, dt, events)`, captain API, radar/scanner queries; needs: LOS queries from wp_world (request if missing), AI driving controller, player roles from wp_net; contract requests: `HandlingEffects.tire_puncture`, `PoliceEvent` payload definition).
