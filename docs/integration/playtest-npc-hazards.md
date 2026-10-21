# Playtest fix: "NPC cars are default hazards on low LOD"

- **Who:** helper for the integration lead, 2026-10-21.
- **Crates touched:** `wp_sim` only (lead-owned, minimal). Not touched: `wp_police`,
  `wp_contracts`, `data/`.

## What was wrong
`Sim::rebuild_police` built `PoliceSetup` with `..Default::default()`, and
`PoliceSetup::default()` has `hazards: true`. So every police unit on `wp_police`'s
kinematic driver (no police car model set, or the vehicle limit is close) rolled the
statistical "background traffic hazards" stand-in: random near misses and phantom
collisions that cost health and cut speed to 30%. That model exists for headless runs
without real traffic; in `wp_sim` the civilian cars are real `wp_world` agents and are
already reported to the units as obstacles every tick, so the units were paying for
traffic twice, and the invisible half of it was dice rolls.

## Change
- `crates/wp_sim/src/police_link.rs`: `hazards: false`, with the reason in a comment.
- `crates/wp_sim/src/lib.rs`: test `kinematic_police_do_not_roll_phantom_traffic_hazards`
  (60 s city pursuit with kinematic units: zero stand-in near misses and collisions; fails
  with the old setting).

## Requests
- **Police (optional):** consider defaulting `PoliceSetup::hazards` to `false` so the stand-in
  is opt-in for headless tools (simcli and the tuning-dash probe already set it explicitly).
  `wp_opponent/tests/common` still inherits the default `true`.
