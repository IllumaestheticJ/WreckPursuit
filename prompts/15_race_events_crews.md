# PROMPT 15 — Race types, event scheduler, crews & territory (`wp_race`)

Read `CLAUDE.md`, `docs/INTEGRATION.md`, `docs/CONTRACT_CHANGELOG.md`, `docs/DESIGN_OVERVIEW.md`. You own a new crate `wp_race` and `data/events/`. Branch `ws/race`. `wp_net` (prompt 07) owns the lobby transport; you own the rules. Read `wp_world::routes` and `wp_economy` payout API; do not edit them.

## Goal
The content layer of the open world: what races exist, when, with what rules, how results are judged, and the crew/territory meta that gives night racing stakes beyond cash.

## Work
- Race types with rule objects: `Sprint`, `Circuit(laps)`, `Outrun` (1v1 gap race with a lead distance to win), `Checkpoint` (open-route, any path, timed), `Cannonball` (cross-city, traffic-heavy), `Delivery` (cargo mass added to the car; damage to cargo costs), `Drag` (reaction + shift timing), `TimeAttack`. Each has start-grid rules, checkpoint validation (server-side, tolerance by speed), DNF/DQ rules (total loss, capture, leaving corridor), result scoring, clean-race detection (no contact above impulse threshold, no civilian hits), and a stake table (`data/events/stakes.toml`) keyed by `DayPhase`, type, class bracket.
- Event scheduler: generates the day's calendar from seeded rules — sanctioned daytime circuit events on closed routes (traffic suppressed, cones, marshals as static entities), frequent night meets at POIs (spawn, lobby window, start), and **rare illegal daytime street races** (very high purse, flagged `police_presence_override: max`). Meets have a rolling heat cost: too many races at one POI attracts a patrol.
- Crews: create/join, shared rep, crew bank cut, crew-vs-crew challenge races (pink-slip optional), territory = districts with control points earned by wins; controlled districts give a small heat decay bonus and a safehouse. Territory decays weekly.
- Progression gates: class brackets by `class_index`, licence tiers unlocked by clean-race counts, event entry requirements.
- Deterministic simulation of results from scripted race traces for tests; property tests on scoring (no ties unresolved, DQ ordering, stakes monotone with risk).

## Deliver
Crate, data, README (design write-up), `docs/integration/race.md` (API `wp_net` calls to create/start/judge races, events it emits to `wp_economy`/`wp_police`, contract requests for a `RaceEvent` payload).
