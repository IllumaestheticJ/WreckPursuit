# Traffic hard impacts integration notes

- **Workstream:** crashed civilian traffic stops with hazards on (owner ask, 2026-10-24)
- **Branch:** `claude/project-thread-b88ohu`
- **Crates touched:** `wp_world` (traffic), `wp_sim` (lib step, traffic_link tests)
- **Contract version built against:** current main (no contract change)

## What changed
A physical civilian traffic car that takes a hard hit now stops, puts its hazards on and
stays stopped, the same as one with a dead engine or a lost wheel.

- `wp_world::traffic::HARD_IMPACT_DV_MPS = 5.0` (18 km/h velocity change in one tick).
- `Traffic::report_impact` / `World::report_agent_impact(agent, dv_mps)` disables the agent at
  or above the threshold.
- `wp_sim` sums each traffic car's contact impulses (normal + friction) every tick after the
  contact filter, divides by the chassis mass and reports it. Police, racers and players are
  not touched.
- Judged on impulse, not damage, so Arcade and Simulation behave the same (owner rule
  2026-10-20). Test `hard_impacts_stop_traffic_the_same_in_both_damage_modes`.

Measured on the small procedural city (debug_sedan into debug_sedan, rear-end): closing
3 m/s → Δv 1.2, 8 m/s → 4.6 (drives on); 10 m/s → 5.8, 16 m/s → 9.0 (stopped, hazards).
So a rear-ender at roughly 35 km/h over the other car's speed, or a wall at 18 km/h, wrecks it.

The testbed already blinks hazards on promoted cars from the agent flags; no renderer change.

## Scenario impact
- `jam_pursuit_grid`, `jam_pursuit_vesper`: hashes change (struck cars stop instead of driving
  on); goldens re-blessed.
- `jam_pursuit_vesper` first failed its row 85 gates (max stop 114 s, queue 18). No wreck was
  involved: the changed chase left police unit entity 2 parked in a junction box at about
  (-8, 153) from ~350 s to the end, and the civilian already in the box behind it (agent
  2076) could only try to back out into the queue behind it, which kept refilling. The queue
  spilled ~100 m back through the (0, 50) junction.
- Fix (traffic side, police unchanged): a car inside the box behind a tracked vehicle that has
  stood for `BOX_SQUEEZE_S` = 5 s edges around it (`BOX_SQUEEZE_MAX_M` 3 m, 0.4 m clearance,
  2 m/s) and does not back out meanwhile. Vesper passes again; grid's pending gates are as
  before (queue 14, was 15 on main).
- Test `wp_world/tests/blocked.rs::a_car_parked_in_a_junction_box_is_edged_around`: longest
  stand behind the parked car 5.2 s (48.6 s without the squeeze), no contact.

## Changes outside my crates
None (wp_world and wp_sim only).

## Known gaps
- Kinematic (far) traffic has no collider and cannot be hit; unchanged.
- A car still at the stop line (not yet in the box) behind a parked vehicle keeps the old
  behaviour: another exit, or turn around / back out from the tail of the queue.
