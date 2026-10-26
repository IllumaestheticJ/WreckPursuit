# Nitrous integration notes

- **Workstream / prompt:** owner request 2026-10-26 ("give the HUD and cars and AI a nitrous mechanic. Allow nitrous to recharge faster and stronger for police and opponents as a way of allowing rubberbanding")
- **Branch:** `claude/project-thread-5ajueb`
- **Crates / folders owned:** `wp_vehicle::nitrous`, `wp_sim::nitrous_link`, `data/tune/nitrous.toml`
- **Contract version built against:** 0.4.x (no contract edits)

## What it does
Every car carries one nitrous bottle (`wp_vehicle::nitrous::Nitrous`). The driver fires it with
`VehicleInput::tool_use == NITROUS_TOOL (1)`. It burns only with the throttle at least
`bottle.min_throttle` in a forward gear with the engine running; while it burns the engine makes
`torque_gain × scale.strength` more torque (it goes through `power_scale`, so damage and heat
still cut it). After `refill_delay_s` without burning it refills at `scale.refill / refill_s` per
second. An emptied bottle re-arms at `rearm_fill`.

Rubberbanding: `wp_sim::nitrous_link::update_ai_nitrous` runs every tick before the vehicles
step. AI racers measure the nearest player's distance ahead along their own heading; AI police
(not player cop seats) measure the car their `Assignment::target()` names (pursue / intercept).
The bottle scale goes from `near` (level or ahead) to `far` (`gap_full_m` behind) and the AI
fires while `fire_gap_m..=max_gap_m` behind on a straight with the throttle down
(`AiNitrous::wants_fire`). The fire request is added to the input the vehicle steps with only;
`SimVehicle::input` is unchanged, so session logs and replays keep the AI's own input and the
decision is re-derived deterministically on replay. An AI with no player in range never fires,
so AI-only races and benches keep their old pace.

Nitrous Kit: the garage's `elec_nitrous_1` / `elec_nitrous_2` (`ToolLoadout.nitrous`) upgrade
the player's stock bottle by `[kit]` (strength and capacity). `Career::nitrous_tier` reads the
tier with every mod counted (preview mode `None`) and `car_session` sets it with
`Sim::set_nitrous_scale`, so the kit works the same in Arcade and Simulation even though the shop
lists it as Arcade only. Owner decision card 2026-10-26: recommended "stock bottle on every car,
the kit upgrades it" (implemented while the card was open).

Physics is identical in Arcade and Simulation (owner rule 2026-10-20). The police change is the
owner's explicit request (the usual "police AI does not change" rule is waived for this).

## Exposes
- `wp_vehicle::nitrous::{NITROUS_TOOL, NITROUS_TOML, tune(), NitrousTune, BottleDef, AiNitrous, Band, NitrousScale, Nitrous}`
- `Vehicle::{nitrous(), set_nitrous_scale(), set_nitrous_fill()}`, `Telemetry::{nitrous_fill, nitrous_on}`
- Vehicle snapshot format 14 (adds the bottle; format 13 snapshots no longer restore).
- WASM `nitrous(entity) -> [fill, burning, strength, refill]`; `set_input` tool slot 1 = nitrous.
- Testbed: `nitrous` binding (N, pad B, touch 🔥 button), HUD gauge line "N₂O" under the rpm bar.

## Contract change requests
| # | Change (type / field / variant) | Why | Who is affected | Status |
|---|---|---|---|---|
| N-1 | Document `VehicleInput::tool_use == 1` as nitrous for every role (police tools start at 2) | Nitrous rides on the existing tool slot so it reaches logs, replays and the net input without a new field | wp_net (already carries tool_use), any future player cop tools | open |

## Changes outside my crates
- `wp_wasm`: the `nitrous()` export; `Career::nitrous_tier` + the kit scale at spawn.
- `apps/simcli/scenarios/opponent/pace_grid_highway_sprint_s50.ron`: ai_vic / player race-time ranges re-locked (AI burn nitrous behind the AI-driven player seat).
- Goldens re-blessed (police pursuits: city_chase, jam_pursuit_grid / _vesper, sideswipe_patrol_day, tunnel_escape_founders_cut (+ _control), fog_pursuit (+ _clear)); every range gate held.
- `apps/testbed-web`: binding, input, touch button, HUD gauge.

## Known gaps
- The racer gap is straight-line along the heading, not race progress: an AI a lap down on a loop, or on a parallel road, can read a player as ahead.
- No nitrous sound or exhaust flame yet.
