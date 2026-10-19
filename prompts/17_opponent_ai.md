# PROMPT 17 — Racer opponent AI (`wp_opponent`)

Read `CLAUDE.md`, `docs/INTEGRATION.md`, `docs/CONTRACT_CHANGELOG.md`. You own a new crate `wp_opponent`. Branch `ws/opponent`. You may read `wp_vehicle` (shared AI driving controller — reuse; if it isn't merged yet, implement a minimal pure-pursuit + PID controller behind a trait and note it), `wp_world::routes`, `wp_race` rule objects.

## Goal
AI street racers that race *cleanly and believably* in a game where contact is expensive: racing lines from route geometry, braking points from the car's actual measured capability, overtaking only with margin, defending without ramming, mistakes that scale with personality and pressure, and no rubber-banding — difficulty comes from car and driver skill parameters, not cheats.

## Work
- Racing line: from a route's lane polylines build a corridor, then optimise a line (minimum curvature with corridor constraints, iterative) and a speed profile from the car's `wp_vehicle::metrics` (max lateral g, braking g) with a per-driver safety factor.
- Driver model: `DriverProfile { skill, aggression, consistency, risk, awareness }` → lookahead, braking-point error (seeded noise), throttle smoothness, willingness to use traffic gaps, tolerance for side-by-side.
- Tactics: slipstream, overtake decision (closing speed, gap, upcoming corner, traffic ahead), defensive single move, yield when losing the wheel, avoid civilians at all costs (a civilian hit is a race DQ for AI too), rejoin logic after off/contact, damage awareness (slow down when `HandlingEffects` say the car pulls or overheats).
- Pursuit awareness: when police heat is active (query hook), racers may abandon the race and run; configurable.
- Tests on the procedural city: 8 AI racers × 5 laps circuit with zero AI-AI collisions above impulse threshold at default aggression; lap-time spread matches skill ordering; determinism hash.

## Deliver
Crate, README (tuning parameters), scenarios, `docs/integration/opponent.md`.
