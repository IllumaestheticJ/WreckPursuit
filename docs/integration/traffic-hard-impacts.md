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

## Changes outside my crates
None.

## Scenario impact
- `jam_pursuit_grid`: hashes change (struck cars stop instead of driving on); gates pass.
- `jam_pursuit_vesper`: hashes change and the row 85 gates fail (max stop 114 s, queue 18).
  Traced: no wreck is involved. The changed chase leaves police unit entity 2 (siren off)
  parked in the junction box at about (-8, 153) from ~350 s to the end; the civilian behind
  it in the box (agent 2076) waits there for good and the northbound queue spills ~100 m back
  through the (0, 50) junction. Wrecks from this change are all despawned out of sight well
  before. On main the same scenario passes, so it is the chase course, not the jam logic,
  that changed.

## Known gaps
- Kinematic (far) traffic has no collider and cannot be hit; unchanged.
- Runs where a traffic car is hit hard now diverge from before (the car stops instead of
  driving on); any golden hash covering such a hit needs re-blessing.
