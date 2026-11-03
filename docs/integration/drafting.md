# Drafting (slipstream) integration notes

- **Workstream:** lead — drafting (owner approval 2026-10-31: "is there a drafting mechanic
  yet" → "yes" to enabling it). The one approved exception to the physics freeze, limited to
  the slipstream.
- **Branch:** `lead/drafting` (from `main` 1abfd2a4)
- **Crates / folders touched:** `wp_vehicle` (model + aero hook), `wp_sim` (per-tick pass),
  `wp_opponent` (re-export), `wp_wasm` + `apps/testbed-web` (HUD readout), `apps/simcli`
  (scenario).
- **Contract version built against:** 0.4.2 — **no contract change**.

## What it does
Every fixed tick (`Sim::step_fixed`, right after AI nitrous and before any car steps) the sim
builds one `DraftBody` per vehicle slot from its Rapier body (x/y position and velocity) and
`VehicleDef::body_size_m` (width, length), runs `wp_vehicle::draft::drag_scales_into` over all
of them and gives each car its multiplier with `Vehicle::set_draft_drag_scale`. The vehicle
multiplies its aero drag by it on top of the damage model's `HandlingEffects::drag_scale`
(`aero_forces(.., eff.drag_scale * draft_drag_scale, ..)`).

**One physics for every car and both modes** (owner rule): players, AI racers, AI police,
player cop seats, physical traffic and network players (the server runs the same
`Sim::step_fixed`) all draft and all make a wake. `draft_link.rs` never looks at
`VehicleKind` or `DamageMode`. Dormant cars stand still (no wake); light-mode cars make a wake
from their kinematic body but have no aero step of their own. Tested: same tow bit for bit in
Arcade and Simulation and with Player / AiRacer / Police / Traffic cars.

Model (unchanged from `wp_opponent::draft`, now `wp_vehicle::draft`): 35 % less drag bumper to
bumper, linear to nothing 25 m behind the leader's rear bumper, fading quadratically to the
edge of a wake one leader width wide (+50 % over the range); leader ≥ 10 m/s, follower within
~25° of its heading; strongest leader wins, no stacking.

## Layering
`wp_opponent` depends on `wp_sim` (dev) and on `wp_vehicle`; `wp_sim` depending on
`wp_opponent` would invert the graph (AI above the loop that drives it). The model is pure
physics over plain arrays, so it moved to **`wp_vehicle::draft`**, next to `aero.rs`.
`wp_opponent::draft` re-exports it (`drag_scales`, `DraftBody`, `DraftParams`, …) so callers
keep compiling; `DraftBody::from_obs` became the free function
`wp_opponent::draft::body_from_obs` (an inherent impl cannot live outside the type's crate; it
had no callers outside the old file). The model's tests moved with it. `wp_sim` needs no new
dependency (it already depends on `wp_vehicle`).

## Exposes
- `wp_vehicle::draft::{DraftBody, DraftParams, DraftScratch, reduction, drag_scales,
  drag_scales_into, drag_scales_all_pairs}`.
- `wp_vehicle::vehicle::Vehicle::{set_draft_drag_scale, draft_drag_scale}` (1 = clear air;
  derived each tick, **not** in `VehicleSnapshot`, not hashed, not replicated: replays and the
  server recompute it from the states).
- `wp_sim::DRAFT_PARAMS` (the tuning the sim applies = `DraftParams::default()`),
  `wp_sim::Sim::drafting(id) -> Option<f32>`: drag reduction 0 .. 0.35.
- `wp_wasm`: `drafting(entity) -> f32` (same value), `Session::drafting`; testbed
  `sim.drafting(entity)`.
- Testbed HUD (`hud.ts` `slipstreamTag`, style in `index.html`): a small `SLIPSTREAM` tag after
  the gear in the speedo panel, dim grey in clear air, green with the drag cut (`SLIPSTREAM
  −28%`) from 12 % reduction (about 15 m behind a leader).

## Determinism and cost
Fixed order (vehicle slot order in, slot order out). Broad phase: bodies sorted by world x
(`total_cmp`, ties by index); a follower only tests leaders within `reach = range + max
length + 1.25 × max width` in both x and y, slower-than-`min_speed` leaders and non-finite
bodies skipped; per-body speed / direction computed once. The max over candidates does not
depend on their order, so the result equals the all-pairs test bit for bit (unit test
`broad_phase_matches_every_pair`, random packs of 0–64 cars). f32 only, `sqrt` and division
only, no allocation per tick (scratch kept in `Sim`).

`cargo run --release -p wp_vehicle --example draft_bench` (cloud container, release):

| Layout | Cars | µs / physics tick | ms / 60 Hz frame | all-pairs µs / tick |
|---|---|---|---|---|
| race pack (4 abreast, 8 m apart: every pair in reach, worst case) | 24 | 4.0 | 0.016 | 5.7 |
| spread over a 1.2 km city | 24 | 0.6 | 0.002 | 3.2 |
| race pack | 64 | 16.2 | 0.065 | 35.5 |
| spread over a 1.2 km city | 64 | 3.3 | 0.013 | 28.6 |

Against the vehicle budget (0.15 ms/car/frame → 3.6 ms for 24 cars) the pass is < 0.5 %
in a 24-car pack. Building the bodies (one Rapier lookup per car) is not in the table and is
negligible next to it.

## Numbers (Kestrel TS, CdA 0.668 m², flat world)
- 5 m bumper-to-bumper behind a leader, both full throttle from 40 m/s for 4 s: **45.80 m/s
  vs 45.28 m/s alone (+0.52 m/s)**, mean drag cut 28.4 %, gap closes 5.0 → 4.0 m, no contact
  (`crates/wp_sim/tests/slipstream.rs`). Same numbers in Arcade and Simulation.
- Coasting from 40 m/s for 4 s: 37.74 vs 37.30 m/s alone.
- simcli `vehicle/slipstream_tow.ron` (towed, leader, control two lanes over): towed 45.58,
  leader and control 45.07 m/s at 4 s.
- Top speed: in a sustained tow at the full 28–35 % cut a power-limited car's top speed rises
  by (1/0.72)^(1/3) − 1 ≈ 11 % (estimate from P = F·v with drag dominant; not measured, the
  follower runs into the leader first).

## Tests
- `wp_vehicle` `draft` unit tests: wake geometry (moved), broad phase = all pairs, non-finite
  bodies ignored.
- `wp_sim/tests/slipstream.rs`: tow gains speed (both modes), coasting tow loses less speed,
  no effect side by side (3.5 m) or behind a leader under `min_speed`, same tow in both modes
  and for every vehicle kind (bit-equal), determinism (state hash).
- `wp_sim` `draft_link` unit test: `DRAFT_PARAMS == DraftParams::default()`.
- `apps/simcli/scenarios/vehicle/slipstream_tow.ron` (expectation ranges).
- `wp_opponent` race harness (`tests/common`) now measures tow time with `Sim::drafting`
  instead of calling the old proposed `Sim::set_drag_scale` (never merged).

## Changes outside my crates
`wp_vehicle` (02's crate): new `draft` module, one `Vehicle` field + setter/getter, one factor
in the aero call. `wp_opponent` (17's): `draft.rs` is now a re-export. `wp_wasm` / testbed
(04's): one readout function + HUD tag.

## Known gaps
- **Client prediction** (`wp_net::predict`, a lone `Rig`) does not model other cars, so it
  predicts the local car in clear air; reconciliation absorbs the difference (a full tow is
  ~0.15 m/s² at 40 m/s: ≈ 1 mm over a 100 ms round trip, under the 5 cm tolerance).
- `RaceConfig::draft` (the AI's tow *tactics*: closing up, pulling out with the run) stays off
  by default; the physics applies either way. Turning it on is an AI decision for 17 / the lead
  (the 2026-10-21 port that turned it on made `hollins_hill_sprint` never finish, see
  `triage-2.md`). The feature-gated `draft_lets_a_faster_driver_in_the_same_car_pass` stays
  gated.
- No wake visuals or audio; no wake behind trucks being bigger than behind cars beyond what the
  width does.

## Contract change requests
None.
