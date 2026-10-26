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
Closer than `fire_gap_m`, level or ahead, an AI has the player's bottle; from `fire_gap_m`
behind the scale runs `near` .. `far` (at `gap_full_m`). AI fire on a straight with the
throttle down (`AiNitrous::wants_fire`): racers from `fire_gap_m` to `max_gap_m` behind the player, and
level or ahead while the player is within `ahead_gap_m` (40 m) behind them (owner 2026-10-26:
"AI racers can use nitrous if they're ahead (just normal regen)"; a clear leader does not burn,
or the field splits: with every leader burning, `loop9_ashpeak_highway` had 4 of 6 miss the
finish window);
police from `fire_gap_m` behind, and on a call (a target or the siren on) below
`up_to_speed_mps` to get up to speed (owner 2026-10-26: "cops can use nitrous to get up to
speed"). A patrolling cop never fires. The fire request is added to the input the vehicle steps with only;
`SimVehicle::input` is unchanged, so session logs and replays keep the AI's own input and the
decision is re-derived deterministically on replay. An AI with no player in range never fires,
so AI-only races and benches keep their old pace.

Nitrous Kit (owner decision 2026-10-26, "Kit required for the player"): a player's car has no
bottle (`Nitrous::fitted = false`, no HUD gauge) until the garage's `elec_nitrous_1` /
`elec_nitrous_2` (`ToolLoadout.nitrous`) is fitted; tier 1 is the base bottle, tier 2 is ×1.5
strength and size (`[kit]`). `Sim::set_nitrous_kit` sets it: the career from
`Career::nitrous_tier`, simcli / free drive from the entity's `mods`. The tier counts every mod
(preview mode `None`), so the kit works the same in Arcade and Simulation even though the shop
lists it as Arcade only. AI racers and police always carry a bottle regardless (owner,
2026-10-26: "make sure AI gets nitrous regardless of if driver has it - only for the purpose of
catching up").

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
