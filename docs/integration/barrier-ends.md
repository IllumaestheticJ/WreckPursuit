# Barriers: no launches, cushioned ends (2026-10-27)

Owner: "the loop9 k rails launch cars too easily" and "sections of railing that abruptly start
that will cause massive crashes - either fix or put water barrels in front".

## K-rail launches (wp_sim)

The wheel probes (`wp_vehicle::RapierGround`) saw every static collider, barrier and rail meshes
included. A tyre swept onto a K-rail's top edge read ~0.8 m of sudden ground, the suspension
threw that side up and the car rolled or flew over the barrier. `Sim::step` now leaves
`BARRIER_USER_DATA` colliders out of the wheel query filter; the chassis box alone meets
barriers and scrapes along them. Deck rails and sidewalk bare-back rails share the user data, so
the same applies to them.

Measured with `cargo run --release -p wp_sim --example krail_launch` (5 straight median and
edge stretches beside Loop 9, both sides, 20/35/50 m/s × 5/15/30/50°, 240 hits per car; a hit
counts if the car tilts past 45° or spends > 0.1 s with its centre 1.2 m above the barrier foot):

| car | before | after |
| --- | --- | --- |
| debug_sedan | 33 / 240 | 1 / 240 |
| erg_hamada | 77 / 240 | 2 / 240 |
| kestrel_ts | — | 2 / 240 |

The hits left are 30°+ at 35–50 m/s. Regression test: `wp_sim/tests/krail_launch.rs` (Port
Vesper median, 24 hits; it fails on the old filter with 3 flips).

## Water barrel cushions (wp_world::barrier_ends)

`exposed_ends(map)` finds every free end of a median, edge, deck or sidewalk-back rail line that
faces oncoming traffic on a fast highway lane (≥ 14 m/s, within 5.5 m and 30° of head-on, same
level) and does not join another barrier line. `water_barrels(map)` stands a graded cushion in
front of each: rows of 2 barrels of 850/650/450/300 kg from the concrete out, 1 × 150 kg at the
nose (≈ 4.7 t, ≈ 4.8 m deep), shifted up to 0.45 m off the barrier line or put in single file
where a lane is close. `World::new` adds them after the crossover cones, and moves crossover
cones that would stand in a cushion out past it (`make_room`).

Ashpeak: 51 Loop 9 ends (8 at crossovers, 36 edge K-rail starts, 7 deck rail starts), 261 in
the world. `cargo run --release -p wp_sim --example loop9_barrier_ends` lists them (ROAD='*').

Physics (wp_sim): `PropKind::WaterBarrel` props are ordinary props (fixed until a car comes near)
in `BARREL_GROUP`, which pairs with neither other barrels nor the barrier meshes
(`BARRIER_GROUP`). Touching barrels backed by the concrete were a rigid block and stopped a car
as dead as the bare end; now the car picks each barrel up in turn and shoves it along (through
the barrier end if need be). `cargo run --release -p wp_sim --example barrel_crash` drives a car
head-on into the Loop 9 ends with and without barrels: at 40 m/s, peak deceleration (0.05 s
window) ~44 g over ~0.7 s instead of ~78 g in one tick, health left ~0.96 instead of ~0.81.
Rows where both columns match are probe misses (the straight approach meets something else on a
curve), not cushion failures.

Renderer: wp_wasm prop kind code 4 (`KIND_WATER_BARREL`), testbed `PROP_KIND.waterBarrel`, a
yellow instanced cylinder in `render/props.ts`.

Interplay with open PRs: #128's sloped median terminals and #126's edge barrier rework change
the barrier lines; the cushions are derived from those lines, so they follow (re-bless goldens
after merging either).
