# World density integration notes (lead helper)

- **Workstream:** owner reports 2026-10-29: "need twice as much roaming racers on the full
  map" and "parked cars are way heavier than traffic cars"
- **Branch:** `lead/world-density`
- **Crates touched:** `wp_wasm` (`street.rs`, `street_tests.rs`, `lib.rs` doc), `wp_sim`
  (`parked.rs`, `lib.rs`, new `parked_mass.rs` tests, README)
- **Contract version built against:** unchanged (no contract change)

## 1. Twice the roaming street racers on the full map
**Cause.** The street-racer roster was one fixed table for every city (`street::ROSTER`, 13
racers: 8 all day, 1 night roamer, 4 Loop 9). The full world map (`ashpeak`, Port Vesper +
Ashpeak County) is several times Port Vesper's road network, yet got the same 13, so the
county was thin.

**Change.** `street::FULL_MAP_ROSTER` (26 = 2 × 13) and `street::roster_for(world)`, which
picks it when `world.network().id == wp_world::ashpeak::CITY_ID` (every other city keeps
`ROSTER`). The first 13 entries are `ROSTER` itself; the other 13 roam: 8 all day (two per
level) and 5 at night. Loop 9 keeps its 4 (it is one highway inside Port Vesper; doubling it
would only double Port Vesper's highway). `plan_roster` and the lockup (`lockup_slots(world,
n)`) size from the roster; `NAMES` / `TEMPERAMENT` grew to 26 names (entity ids stay unique).
Spawning is unchanged: lanes picked uniformly over the map, ≥ `SPAWN_SPACING_M` (140 m) from the
player and each other; moves later in the session (shift changes, respawns) still go only where
the player cannot see (`out_of_view`: ≥ 150 m and not in the player's view).

| Full map | Before | After |
|---|---|---|
| Racers in the session | 13 | 26 |
| Out by day (noon) | 8 | 16 |
| Out at night (23:00) | 13 (4 on Loop 9) | 26 (4 on Loop 9) |

Per-city spread (test `full_map_spawns_twice_the_racers_spread_over_both_cities`, Port Vesper's
`vesper_v2` bounds, default seed): noon 5 in Port Vesper + 11 in Ashpeak County; 23:00 8 + 18.
Port Vesper stays below what it has as a city of its own (8 / 13); the county carries the
rest. Closest pair 220 m / 198 m (≥ 140 m). The 13-racer roster of the other cities draws its
names from the original 16 (`ORIGINAL_NAMES`), so their sessions spawn exactly as before.

**Frame cost** (`street_perf`, native release, ashpeak, sim ms per 60 Hz frame over 30 s after
a 5 s warm-up; the machine was shared with other builds, so compare the racers' share):

| | racers off | racers on | racers' share | per racer | street AI |
|---|---|---|---|---|---|
| noon, before (13) | 2.21 | 4.04 | +1.82 | 0.140 | 0.193 |
| noon, after (26) | 3.12 | 6.46 | +3.34 | 0.128 | 0.436 |
| 23:00, before (13) | 0.95 | 2.57 | +1.62 | 0.124 | 0.152 |
| 23:00, after (26) | 1.04 | 4.78 | +3.74 | 0.144 | 0.347 |

Cost per racer is unchanged (~0.13-0.14 ms); the total doubles with the count. Vehicle slots:
35 vehicles at noon / 29 at 23:00 out of `MAX_VEHICLES` 64 with physical traffic intact (79 / 39
agents), but every racer, even in the lockup, holds a slot, so `cap_traffic_promotion` leaves
13 fewer slots for physical traffic in a big pursuit.

## 2. Parked cars as heavy as the car they are
**Cause.** A parked car is a prop box (`Sim::insert_prop`): 1400 kg (`PARKED_CAR_MASS_KG`, the
same as the parked model `debug_sedan`, so the mass itself was right) sliding on friction 0.6
in every direction, like a car with all four wheels locked. A stopped traffic car stands on
its tyres: it rolls along its length (only the held axle drags) and grips hard sideways. So a
parked car hit from behind (the common hit) moved half as far as a traffic car of the same
model (5.0 vs 10.2 m on flat ground at 40 km/h), and slid 50% further when T-boned. The
striker's Δv (the momentum exchange) already matched within ~10%.

**Change (prop setup in `wp_sim`, no `wp_vehicle` change, no mode branch).**
- Parked-car boxes slide on `PARKED_CAR_FRICTION` = 0.2 (other props keep 0.6).
- `Sim::parked_side_grip` (before each physics step, active parked cars in ascending prop
  order, from last tick's contacts): an upright parked car on a fixed surface loses up to
  `PARKED_CAR_SIDE_GRIP_MPS2` = 5 m/s² of sideways speed per second, and the matching yaw rate
  (the same grip at the axles on the box's yaw inertia); never past zero.
- `Sim::set_parked_car` gives every parked box the model's `mass_kg`.

**Measured** (`parked_mass::tests`, debug sedan into a parked debug sedan vs a stopped traffic
debug sedan holding `leader::held` brakes, 40 km/h; striker Δv, struck car's shove after 3 s):

| set-up | Δv parked / traffic, m/s | shove before (parked / traffic), m | shove after, m |
|---|---|---|---|
| flat rear-end | 5.56 / 6.08 | 5.02 / 10.24 | 8.08 / 10.24 |
| flat T-bone | 6.51 / 6.36 | 5.15 / 3.42 | 3.45 / 3.42 |
| Vesper kerb T-bone ×3 | 5.86-6.48 / 5.90-6.22 | 5.55-6.26 / 3.21-4.24 | 3.52-3.76 / 3.21-4.24 |
| Vesper kerb rear-end ×3 | 5.56-6.15 / 6.03-6.17 | 5.26-7.33 / 8.52-10.94 | 7.32-10.44 / 8.52-10.94 |

(Arcade; Simulation gives the same physics and within a few % the same numbers.) Δv within
12%, shove within 21%, the struck car's dents within 32%.

## Tests
- `wp_sim` `parked_mass::tests::parked_car_takes_a_hit_like_a_stopped_traffic_car` (new; flat +
  3 Port Vesper kerbside spots × rear-end / T-bone × arcade / sim; asserts Δv and shove within
  25%, the struck car's plastic energy within 35%).
- `wp_wasm` `street_tests::full_map_spawns_twice_the_racers_spread_over_both_cities` (new).
- `street_perf` (ignored) now also runs `ashpeak` noon and 23:00.

## Known gaps
- Flat-ground T-bone only: the *striker* dents 3× more against the parked box than against the
  traffic car (Simulation 26 vs 8.5 kJ; it was 46 vs 10 kJ before this change). A square hit on
  the box's flat side meets an undented face until the lattice's hull is rebuilt; on Port
  Vesper's kerbside T-bones the striker's dents match (within 30%). Not asserted.
- Parked cars do not roll over kerbs on wheels: a parked car shoved across a kerb slides on its
  box.
