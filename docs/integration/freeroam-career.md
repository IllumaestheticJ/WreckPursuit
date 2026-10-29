# Free Roam in the career: waypoints, where runs start, taxis

Owner reports, 2026-10-29, all about the career's Free Roam.

- **Workstream:** lead helper, branch `lead/freeroam-career`
- **Crates / folders touched:** `wp_wasm` (`career.rs`, `garage_visit.rs`, `session.rs`,
  `lots.rs`, `lib.rs`, `spawn_tests.rs`), `wp_economy` (doc comment on `car_spots` only),
  `apps/ui-kit` (`garage/access.ts`, `garage/CareerGarage.tsx`, `garage/types.ts`),
  `apps/testbed-web` (`ui/waypoint.ts`, `ui/street.ts`, `scene.ts`, `main.ts`, `wasm/sim.ts`,
  e2e `tow.mjs`, a comment in e2e `run.mjs`).
- **Physics:** untouched. **Contracts:** untouched. The sim step is untouched: the run's start
  pose is decided before the session is built and is part of the recorded scenario.

## 1. "need to be able to set waypoints in free roam too"

**Before:** the waypoint (`CareerWaypoint`, `apps/testbed-web/src/ui/waypoint.ts`) could only
be set from the career screen ("Waypoint" on a lot, shop or home row). The city map (M) had no
picker outside a street-race challenge.

**Now:** in a career run, while the full city map is open (and the street-race finish picker
is not using it), the map has a waypoint picker:
- a click near a place icon (within 45 m: repair shop, safehouse, car lot, home garage) marks
  that place, the same target the career screen's "Waypoint" sets;
- a click anywhere else drops a **road pin** on the nearest drivable lane (new wasm
  `road_point(x, y)` → `Session::road_point`, `garage_visit::snap_to_lane`);
- a click on the current waypoint (or the place again), or the bar's **Clear**, clears it.
A bar at the foot of the open map says how (`data-testid="map-waypoint"`).

The waypoint shows as before on the minimap and the full map (flag + GPS line) and now also in
the world as a gold beam with a ground ring (`SceneView.setWaypoint`). A road pin clears when the
car gets within 20 m; a place clears when the car pulls in (as before). It is one object behind
`CareerApi.waypoint()` / `setWaypoint()`, so the career screen and the map always agree. The
`CareerApi` types grew a `WaypointTarget` (place kinds + `"Road"` with `pos`), and
`setWaypoint(null)` clears. The career screen's head shows "Waypoint: X · Clear", and a
"Waypoint set" button clears when pressed again.

## 2. "when im towed to meridian motorworks and continue free roam there - i dont start there"

**Root cause:** `Career::start_run` always put the player at the chosen home garage's anchor
(`garage_visit::home_spot(&self.home_id())`), ignoring `CareerProfile.car_spots`, which the tow
fills with the shop or garage the car was taken to. The session then spawned on the home lane,
inside the home ring (its panel popped up at once).

**Rule now:** a run (Free Roam / night run in the combined world; races keep their grid) starts
**where the active car is**: its `CarSpot` when it is a repair shop or a home garage, else the
chosen home garage (`Career::start_spot` / `start_place`). A wreck on the road cannot start a
run anyway. Starting the run still clears the car's spot (the car drove off).

**Spawn:** `garage_visit::run_spawn` puts the car on the drivable lane by the place's ring,
24–54 m along the road from it (ahead first, then behind on the lane running the other way),
facing along the lane away from the place, and outside **every** place ring by 4 m
(`all_rings`: shops, safehouses, lots, homes). So no place panel opens at the start. The career
hands the place to the session as the scenario position (the place's anchor, found again by
`start_place_at`); if no lane point qualifies the old lane-at-the-place spawn is the fallback
(`spawn_tests::every_start_place_has_a_spawn_outside_the_rings` checks none of the 5 shops and 3
homes needs it). Measured: Meridian Motorworks 24 m.

**Career screen:** `CareerView.start_at` (`SpotView`) → "Next run starts at: X"
(`data-testid="career-start-at"`).

## 3. "i also can't taxi to dealers once i own a car"

**Root cause:** the career screen's lot rows (`LotsTab`, `CareerGarage.tsx`) offered "Taxi" only
through `canWalkLot(place, lot, hasDrivableCar)`, which was true only at that lot or **without**
a drivable car; with one, the row showed only "Waypoint". The host never refused (no Rust guard).

**Now:** `lotActions` (`access.ts`): at the lot (or a host that does not track places) "Visit";
anywhere else always **"Taxi"**, plus "Waypoint" when the player has a drivable car. The taxi is
a new host call `visitLot(id, taxi = true)` → wasm `career_taxi_to_lot` →
`Career::taxi_to_lot`: the same lot visit as driving in, without the player's car at the gate.

**Car location rule (taxi):** the owned car **stays where it is**. Its spot is untouched, so the
next run starts from the car (item 2): towed to Meridian Motorworks, taxi to Dockside Salvage,
browse, back to the garage, Free Roam → starts by Meridian Motorworks. A car bought there has no
spot, so it is at the chosen home garage (where a run with it starts). Driving into a lot (the
place panel's "Walk the lot") still parks the car at the gate, as before.

## Tests

- `wp_wasm` (`spawn_tests.rs`):
  - `free_roam_starts_at_the_shop_the_car_was_towed_to`: tow to Meridian Motorworks, Free Roam
    spawns < 60 m from the shop, > 15 m (outside the ring and every other ring), on a lane
    (< 3 m), facing along it, no place visit; `start_at` names the shop, then home after the run.
  - `free_roam_starts_at_the_home_garage_the_car_was_towed_to`: the same for a tow to the
    Harnell barn while the chosen garage is the Vesper lock-up.
  - `a_taxi_to_a_lot_leaves_the_owned_car_where_it_is`: with a drivable car, taxis to three
    lots (no car at the gate, spot unchanged); the next Free Roam starts by the shop.
  - `every_start_place_has_a_spawn_outside_the_rings`: every shop and home.
  - `career_run_starts_in_the_chosen_garage`: updated to the new rule (by the garage, outside
    its ring, no visit at the start).
- `apps/ui-kit` vitest: `lotActions` (replaces the `canWalkLot` test).
- e2e `apps/testbed-web/e2e/tow.mjs` (extended): "Next run starts at" after the tow; Taxi +
  Waypoint on a lot with a drivable car; the taxi visit has no own car and the car stays at the
  shop; Free Roam starts 24 m from the shop with no panel; a map click pins a road waypoint
  (career API + beam agree); Clear; a pick on a place icon marks it, again clears.

## Known gaps

- Pulling into a shop or home by driving and ending the run there does not record a `CarSpot`
  (only tows do), so the next run starts at the chosen garage. The rule above is per `CarSpot`.
- `apps/ui-kit` vitest `src/data/data.test.ts > estimator port > protection adds mass and
  durability` fails on this base (unrelated: car data estimator, untouched here).
