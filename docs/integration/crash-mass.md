# Crash mass transfer integration notes

- **Workstream:** owner bug 2026-10-26, "I'm in a much heavier ERG Shimada hitting NPC cars and
  I'm the one getting thrown around" (the car is the Erg Hamada, 2113 kg)
- **Branch:** `claude/project-thread-9lg8ge`
- **Crates touched:** `wp_vehicle` (`rig::ground_collider`), `wp_sim` (`physics.rs`, new
  `crash_mass.rs` tests)
- **Contract version built against:** 0.4.1 (no contract change)

## What was checked
Every path that could give a crash the wrong mass:
- Physical traffic is a normal `wp_sim` vehicle (`Sim::embody_agent` → `spawn_vehicle`): dynamic
  body, mass and inertia from the car file (`Vehicle::build_body`, collider density 0). Not
  kinematic, no mass override, no player-only impulse scaling. Light mode (kinematic) and
  dormant (fixed) bodies are AI racers far from every player only.
- `wp_damage::scale_racer_contacts` (Arcade) scales the impulses the **damage model** sees for
  racers; it does not touch the Rapier response.
- Mods: every mass effect lands in `def.mass_kg` (`wp_economy::mods`), so the body has it.
- Measured on flat ground, Hamada vs Tansy 13 (1058 kg), rear-end, offset, T-bone, head-on, both
  directions, both modes: the velocity changes while the cars touch are 1 : 2 (= the mass ratio)
  in every case but one. On Ashpeak with real physical traffic (beater 1048 kg) the ratio is
  2.0 in rear-ends and T-bones.

## The bug
The one bad case: Arcade T-bone at 15 m/s on flat ground. The Hamada lost 10.96 m/s and the Tansy
gained 10.36 (ratio 0.95 instead of 2), and the Hamada was thrown up at 1.24 m/s and spun.
Cause: the ground was a 10 km × 10 km **cuboid** slab. Its corners sit at ±5000 m, where an f32
ulp is ≈ 0.5 mm, so Rapier's GJK between a chassis hull and the slab cannot place a contact that
is within half a millimetre of the surface. A crushed hull's bottom corner touching the road got
a contact normal of (0.70, −0.70, 0.14): a wall. Its 13 kN·s impulse stopped the struck car dead,
the heavy car ran into it and rode up, and its own corner then hit the same kind of "wall".
A 200 m slab made the bad contacts disappear, which confirmed the precision cause.

## Fix
`wp_vehicle::rig::ground_collider` is now a half-space (exact plane z = 0, closed-form contacts
and queries in Parry). The rig and `wp_sim` keep sharing it, so client prediction parity holds.
Parking it as the safety floor under terrain cities now sets the plane height directly
(`SAFETY_FLOOR_Z`). `GROUND_HALF_EXTENTS` is gone (it had no other users).

Where it matters: flat cities (procedural grid, dev flat ground, carpark tests) drive on this
ground. Cities with terrain (Port Vesper + Ashpeak) drive on heightfield and road meshes, where
the slab is only the safety floor at −60 m; there the crash response was already mass-correct.

## Tests
`crates/wp_sim/src/crash_mass.rs`: heavy into light and light into heavy, five crash shapes,
Arcade and Simulation. Each asserts the struck car's velocity change over the striker's is
within 15 % of the mass ratio and the striker is not thrown up faster than 0.5 m/s. It fails on
the old slab (Arcade T-bone ratio 0.95) and passes with the half-space.

## Changes outside my crates
`wp_vehicle/src/rig.rs` (ground collider) and `wp_sim/src/physics.rs` (safety-floor placement).

## Known gaps
Road and terrain meshes far from the origin (Ashpeak's metro is ~4.6 km out) have the same f32
magnitude. No bad contact showed up there in these crash tests, but meshes are not immune; if
it ever does, the fix is the same idea (collider-local vertex coordinates per tile).

## Re-blessed (`[bless]` commit)
The plane changes flat-ground wheel probes and chassis contacts by float noise, so every
flat-ground determinism record moved; terrain-city goldens (Port Vesper / Ashpeak) did not:
- simcli goldens (`just bless`): the flat-ground scenarios.
- `data/stats/cars.json` (`just car-stats`): menu stat bars, within ~2 % (top speed of the
  fastest cars rose a little: they no longer near the slab's 5 km edge in the metric).
- `wp_opponent` capability table (`WP_WRITE_CAPS=1`): merlo_20t, morrigan_55.
- Pinned hashes: `wp_vehicle/tests/differentials.rs`, `wp_vehicle/tests/slide_control.rs`.
- `tools/specsheet/ram_probe` vmax-trace keeps its old 5 km stop as a local constant.
