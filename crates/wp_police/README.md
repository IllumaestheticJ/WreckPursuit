# wp_police

**Owned by workstream 06 (police & pursuit AI).** Data: `data/police/*.toml`.

Heat/bounty, day/night presence and dispatch, patrol and pursuit AI, police tools (roadblocks,
spike strips, helicopter), racer gadgets (radar detector, scanner) and the captain interface.
Pure Rust, no renderer, builds for `wasm32-unknown-unknown`.

## Design pillars
- Chases are **lethal to both sides**. Nobody is invincible: units use the same damage model and
  spend a **risk budget**, which grows with the wanted level.
- Since round 13 (owner playtest rows 63 / 64, "not aggressive enough", "backup doesn't
  converge") the police **drive at the car's limit and escalate by wanted level**: L1 tail close
  and box in, L2 PIT as soon as the road allows and ram the rear quarter, L3+ hard rams, side
  slams, rolling blocks and junctions blocked ahead. Backup converges on the racer's predicted
  position at once. This departs from the PVPD doctrine's "contain, don't race" on purpose
  (see `docs/integration/police.md`).
- Racers win by **breaking line of sight** for the escape time, reaching a **safehouse**, or
  disabling pursuers (which raises heat and bounty).
- **Day vs night is tables, not code**: every phase-dependent number is a `[x.dawn/day/dusk/night]`
  table in `data/police/`. Damage mode changes only what contact does to the cars.

## How it works (one tick)
`PoliceSystem::step(world, racers, events, dt, out)` is called once per physics tick by `wp_sim`.

1. **Racers** are synced (position/velocity); spike strip crossings are checked per wheel.
2. **Events**: `TrafficViolation` adds heat (full if a unit/camera *witnessed* it, 20% otherwise
   as a civilian *report*). If no unit saw it (only a camera, or nobody), a report incident opens
   at the violation and the nearest free unit is sent after the report delay; `TotalLoss` of a wanted racer is a capture; `Contact` between a unit
   and a racer marks the unit, civilian–civilian contacts above a speed create crash incidents.
3. **Driving** (every tick): internally driven units integrate with the built-in kinematic driver.
4. **Think** (`think_hz`, default 10 Hz): line of sight → rules (heat decay, escape, capture,
   safehouse) → health check (disengage/pull over/wreck) → patrol pool → incidents →
   reinforcements → helicopter → AI captain tools → tactics and drive commands.

### Heat & wanted levels (`heat.rs`)
- Heat 0–100, levels L1..L5 at `level_thresholds`.
- Decays only when unseen and not pursued, after `decay_grace_s`, at
  `decay_per_s[phase] × decay_district_scale[district]` (faster at night, Suburb, Industrial).
- While pursued and seen: `pursuit_heat_per_s` (long chases escalate). Disabling a cop: `cop_disabled_heat`.
- **Pursuit start**: a wanted racer kept within `recognition_range_m[phase]` with line of sight
  for `recognition_time_s[phase]` (a glimpse at night is not enough).
- **Assaulting an officer** (`[heat.assault]`, playtest row 24): a racer hitting a police car.
  Physics contacts (`SimEvent::Contact`) between a racer and a unit's entity are summed into a
  contact episode (contacts less than `episode_s` apart). It counts when the episode's impulse
  reaches `min_impulse_ns`, the cars' relative speed `min_rel_speed_mps`, and the racer's own
  speed `min_racer_speed_mps`. Speeds are the racer's just before the episode's first
  contact (by the time a contact is reported, a head-on hit has stopped the racer, and a patrol
  car nudging a parked racer has shoved it). Then heat rises by `heat`, to at
  least `min_level` (L2), the radio says "Unit N, I've been hit! … assault on an officer.", and
  the pursuit starts in the same tick with the hit unit (or the closest unit within
  `nearby_units_m` that sees the racer, if the hit unit is out of action), day or night: no
  recognition window. A new assault on the same unit needs `repeat_cooldown_s`; ramming another
  unit counts again. Not an assault: parking-lot nudges (relative speed below 2.5 m/s), a
  standing racer hit by a unit, the unit's own PIT / box-in / rolling-block contact on its
  target, and the headless units' proximity stand-in (no collider). Measured contact numbers
  are in `heat.toml`.
- **Escape**: no unit, camera or helicopter has line of sight for
  `escape.base_s[level] × escape.phase_scale[phase]`.
- **Safehouse**: inside a safehouse POI for `dwell_s` with no unit within `clear_radius_m`;
  heat is multiplied by `heat_scale_on_enter`.
- **Capture**: speed < `stop_speed_mps` for `stop_time_s` with a unit within `unit_radius_m`, or
  `TotalLoss` while wanted. Heat resets to 0.
- **Bounty** = `per_heat_second × ∫heat/100 dt + per_race_won_wanted × races + per_cop_disabled × cops`.
  `BountyRecord` is persisted by prompt 08 (`bounty_record` / `set_bounty_record`). It sets
  reinforcement priority (up to `priority_max_reduction` faster response). Night stakes: `payout_scale()`.

- **Blatant speeding** (`heat.blatant`, owner playtest row 60): speeding ≥ 40 km/h over within
  60 m of an in-service unit with line of sight is recognised at once (L1 at least, pursuit by
  that unit), day or night. Otherwise a night pursuit needs 40 m for 4 s, which a fast pass-by
  never gives.

### Presence & dispatch (`dispatch.rs`)
- Pool units per district per phase from `presence.toml [patrols.*]`. Shortfalls are spawned
  (after the session start, out of every racer's sight); surplus idle units retire once out of
  sight and ≥ `response.despawn_min_m` away.
- **Reckless-driving calls** (round 12b, `presence.toml [response]`): a violation no unit saw
  opens a report (civilian call or traffic camera; later calls move it to the racer's latest
  position; a report within 200 m of an open call joins it, so a racing pack is one call).
  After `call_delay_s[phase] + district_delay_s[district]` (Core: 2 s by day, 3 s at night)
  dispatch sends an available unit within `radius_m[district]` by road that can reach a
  junction on the racer's predicted path before it. If there is none, a **responder spawns**:
  `spawn_min_m..spawn_max_m` (150–350 m, + `spawn_lead_s` of a fast racer's travel) of legal
  driving from the racer, never closer than `never_closer_m` (120 m), out of every racer's sight,
  clear of other vehicles and parked cars, facing the racer. Spots that meet the racer on its
  predicted path come first. "Out of sight": no line of sight from the driver's eye or a chase
  camera (behind along the velocity and on every side, since police don't know which way a car
  faces), now or 1 s on, to a car body there.
- **Responders** drive with lights at `respond_speed_factor` (1.6) × limit to the meeting
  junction (re-planned every 3 s) and **hold short of it** while the racer is coming. With the
  racer in sight within 150 m they **follow** it (so it can be recognised, which starts a
  pursuit); with the racer coming head-on within 70 m they pull right and stop, and nose to
  nose with it stopped (a narrow road) they back out 8 m. A unit still > `redispatch_far_m`
  (150 m) away `redispatch_s` (6 s) after taking the call is released and the call goes out
  again. One radio line per call: "Dispatch: caller reports reckless driver, Core. Unit 6
  responding." No responders in a city whose patrol table is empty for the phase.
- **Unit cap**: responders and reinforcements only spawn while ground units <
  `max_units_by_level[highest wanted level]` (and `max_ground_units`).
- **Despawns**: idle responders and reinforcements drive on and vanish once out of every racer's
  sight and ≥ `despawn_min_m` away; out-of-service units the same after 20 s (60 s wrecked).
- **Parked cars and cones** (`PoliceSystem::set_props`, set by `for_world`) are obstacles for
  every ground unit (follow, wait behind, pass), like a stopped car.
- **Crash scenes** (`report_crash_scene`, or automatically from contacts) hold the nearest unit
  for `crash_scene_s[phase]`.
- **Reinforcements**: each pursued racer wants `units_per_level[level]` ground units. A deficit
  creates a request due after `response_s[phase]` (Day 20–40 s, Night 60–150 s). When it comes due,
  an idle unit within 700 m is committed, or a new unit spawns like a responder (fallback: the
  nearest node ≥ `spawn_min_distance_m` away, out of sight).
- **Helicopter**: `count`, `launch_s`, `unavailable_chance` per phase. It requests from `min_level`.
  It is kinematic, gives line of sight to every unit, and is **not** a weapon.
- Every assignment change raises a radio line; `radio::RadioDirector` paces what is spoken as
  `PoliceEvent::Radio` (tiers, sim-time cooldowns, duplicate collapse, earshot;
  `data/police/radio.toml`). Budget: <= 4 lines/min with nobody wanted, <= 10/min in a pursuit
  (`scenarios/radio_*.ron`).
- **One transmission at a time** (playtest row 79): a spoken line holds the channel for its
  length (`words / words_per_s + line_pad_s`, the audio scanner's numbers) plus `min_gap_s`
  (2.5 s); waiting lines are a priority queue (tier, cue `prio`, age), lose their slot when a
  newer line supersedes them ("now level 4" replaces a queued "now level 3") and expire
  (`stale_s`, `stale_critical_s`, or the cue's own life). Gaps shrink with the wanted level
  (`global_gap_s` / `unit_gap_s` per L1..L5; L1 keeps the row-6 8 s / 30 s).
- **Unit-to-unit callouts** (`chatter`, row 79): units call what they see with their call sign
  and a place on the map — "Unit 6, lost visual. Last seen eastbound on Harbor Boulevard at
  Lantern Way.", "eyes on him again", "requesting backup", "I'm on his bumper", "going for the
  PIT" / "he's spun" / "he got away", "boxing him in", "spikes hit", "I'm out of the chase",
  "he's gone into the Hollins Tunnel"; dispatch summarises level rises (place, units on him, air
  unit) and closes pursuits with the place. Places: street (+ cross street at junctions) from
  `World::edge_names` (Port Vesper), tunnel names, district otherwise
  (`PoliceSystem::place_phrase`). The lost-visual call reads the racer record's `last_known`,
  the same position search and interception steer to. Observer only: not hashed, no RNG.
  `chatter_stats()` counts raised callouts. Tests: `tests/radio_chatter.rs`.

Assignments: `Patrol`, `Respond(incident)`, `Pursue(target)`, `Intercept(point)`, `RoadblockAt`,
`SpikeAt`, `HeloTrack`, `OutOfService`.

### Turning round (physical units, lights on)
A unit whose steering target is more than ~107° off its nose (the racer went past it) turns
round: a forward U-turn at full lock from above 3 m/s (handbrake pulled for 0.6 s when fast,
held near 6 m/s) or a reverse swing from a standstill (brake held engages reverse, lock that
swings the nose toward the target), until it faces within 60°; 3 s cooldown. Shadowing units far
behind the racer close at the phase's pursuit cap and brake (4 m/s²) to arrive at their gap.
Tested with real cars in `crates/wp_sim/tests/police_passby.rs`.

### Un-sticking (`stuck.rs`, physical units)
A unit that wants to move (planned > 1.5 m/s, not holding for a light or traffic) but stays
< 1 m/s for 2 s is stuck; so is one that has waited 20 s behind a stopped racer or police car
(not a red-light queue; a civilian jam clears itself). Each event is logged in `stats().stuck`
with a cause from the unit's last contacts: `Damage`, `Vehicle`, `ParkedProp` (the contact point
on a prop footprint), `JunctionStep` / `Kerb` (a low, horizontal contact, near a junction or
not), `Wall`, `Terrain` (vertical normal), `NoContact`. Recovery: back out 4 m with opposite lock
(brake held at a standstill engages reverse) if there is room behind; on a repeat within 15 m
(or held behind a dead car with room behind) re-route with that segment blocked for 30 s, which
turns the unit around; 8 s after the first event, despawn once out of every racer's sight (a
call it had goes out again; a patrol is refilled out of sight). `set_unstick_recovery(false)`
keeps detection and logging only, for measurements. `stats().responses` logs every trip to a
call: spawned or not, start and road distance, stuck events, outcome (`Arrived`, `FellBehind`,
`Stuck(cause)`, `Wrecked`, `CallEnded`).

### Pursuit (`pursuit.rs`, `unit.rs`)
- Pursuers within `CLOSE_LOS_M` (70 m) of the racer **with line of sight** drive the racer's
  own line: its **breadcrumb trail** (recorded only while seen) plus its predicted path. Farther
  away or out of sight they **drive the roads** (`chase_route`: the legal route to the junction
  the racer heads for, then its predicted path), so a unit never follows the trail through a
  corner it would cut into a building (row 66).
- **Chase driving** (`[chase]` in `pursuit.toml`, row 63): corners planned at `cornering_g`
  (0.82 g), braking at `brake_mps2` (8.5), closing at up to the racer's speed × (1 +
  `speed_margin`) and `pursuit_cap_mps[phase]` × `cap_scale[level]`; through junctions at
  `chase_junction_mps`, reds at `chase_red_mps`, a 0.6 s look for cross traffic, passes after
  0.5 s; interceptors at up to 2.2 × the limit.
- **Risk budget**: collisions and near misses cost budget (a deliberate PIT/ram/box contact only
  `tactic_contact_cost`); a unit may spend `risk.budget × risk_budget_scale[level]`, then it
  only shadows at ≤ 1.3× the limit.
- **Tactics by wanted level**, closest pursuer first (civilians within
  `civilian_clear_m[level]` of the racer hold back all deliberate contact):
  - `BoxIn` (L1+): racer < `boxin_max_kmh`, ≥ 2 budgeted units within 60 m.
  - `Pit` (L2+): lead unit, racer < `pit_max_kmh[level]`, ≥ `pit_min_straight_m` of
    straight-ish road ahead; lines up `pit_side_offset_m` beside the racer, turns in at
    `pit_turn_in_gap_m`.
  - `Ram(RearQuarter)` (L2+, lead unit when no PIT is possible), `Ram(Hard)` (L3+);
    `Ram(Side)`: the second unit flanks alongside (L2+) and slams into the racer's flank when
    level with it (L3+). After any deliberate contact a unit re-engages after `reengage_s`.
  - `RollingBlock` (L3+): units ahead of the racer slow in front of it; interceptors that get
    ahead of it on its line join as rolling blocks; at L3+ an interceptor that reaches a junction
    on the racer's path first parks across it (`junction_block_level`).
  - `Shadow`: tail `shadow_min_m..shadow_max_m[level]` back (L1 10–18 m, L2 6–12, L3+ 4–8);
    close to 6 m when the racer stops, ignoring other stopped units (no stand-off short of the
    capture radius).
  - `Intercept`: units off the trail or far behind route to the racer's predicted position
    10–20 s out; interceptors on the same racer try different horizons and keep a route at
    speed rather than flip to one that starts with a U-turn.
- **Backup** (row 64): when a pursuit is on, idle and patrolling units within
  `backup_radius_m[level]` by road (only the racer's district below
  `backup_all_districts_level`) intercept at once, up to `units_per_level` (L1 2, L2 4, L3 6,
  L4+ 8); the rest spawn out of sight ahead of / around the racer after `response_s[phase]`
  (day 4–10 s, night 12–30 s). Units farther than the radius + `far_margin_m` don't count and are
  stood down after `far_release_s`.
- **Search** (row 64): with the racer out of sight, units that reach the last known position
  and interceptors spread over the junctions around it (nearest, the next ones on the path it was
  taking, the roads out of the first), moving on every 10 s. A racer lost inside a covered road
  (tunnel) is searched for at its portals, 30 m outside, each unit at the nearer one (PVPD doctrine §5: nobody chases
  inside), so hiding in a tunnel still breaks contact. Prediction while unseen uses the
  velocity at the last sighting, not the racer's real one.
- **Capture**: stopped (< 2 m/s for 4 s) with a unit within 15 m, a total loss while wanted, or
  **pinned**: never more than `pinned_radius_m` from one spot for `pinned_time_s` with a unit
  within 15 m (a boxed-in racer rocking back and forth).
- **Disengage**: health < `disengage_health` (per mode: Arcade 0.15, Simulation 0.4), a bent wheel (`|toe| > bent_wheel_toe_rad`), a flat
  or a lost wheel, or a dead engine → pull over. Total loss at `total_loss_health[mode]`.

**Driving** uses `wp_world::drive::DriveController`, the controller civilian traffic and opponent
racers share. Each unit owns one, created with its `SteerGeometry` (wheelbase, full-lock angle):
- **Physical units** (`bind_unit_entity(unit, entity, geometry)`): `unit_input(unit, dt)` drives
  with the unit's **pro driver** (`pro.rs`, below) on top of that controller.
- **Headless units**: `step_ground` steers with the same controller, then moves a kinematic bicycle
  body. The body keeps right (`LANE_OFFSET_M`), saturates at a grip limit, crashes when it runs
  wide and has a background traffic-hazard model. Track progress only moves forward
  (`Track::project_ahead`), so hairpins and U-turns don't snap back.

Police keep only the speed planning (corner speeds, caps, tactics).

### Pro driving (`pro.rs`, lead P8)
Physical AI units with lights on (pursuit, intercept, response) drive like trained pursuit
drivers: within ~5 % of the best race AI and (almost) never into a wall, dry or wet. Without
lights (patrol, returning) units keep the calm lane driver they always had (pure pursuit on the
lane, `patrol_cornering_g`, `unit.brake_mps2`). `ProDriver::drive(ctx, track, room, dt)`:
- **Racing line** (lights on, no other vehicle within `traffic_clear_m` ahead): the next
  `window_m` of the track, resampled every `step_m`, its polyline corners rounded
  (`corner_round_passes`), then a lateral offset per sample minimising the summed squared
  curvature (projected Gauss–Seidel, strides 4 → 1) inside the **drivable room**. The window
  starts at the car's own lateral position (no jump on a re-plan, every `replan_s`). While
  passing an obstacle, with traffic ahead or while turning round the unit tracks its lane
  (traffic look-ahead) and only the speed profile is the pro driver's.
- **Room** (`RoadRoom`, `RoomQuery`): built from the city's paved surfaces and lanes
  (`RoadRoom::from_city`; the contract road network's polylines follow a lane, so they are
  not used for widths): own-direction lanes (bus lanes yes, parking no) plus `opposing_m`
  over the centreline, and the whole box of a junction (stop line to stop line). Kept in a
  25 m grid. `RoadRoom::new(&RoadGraph)` is the fallback for hand-built graphs.
- **Speed profile at the grip limit**: `v ≤ √(a_lat/|κ|)` on the line, then a backward pass
  braking inside the friction ellipse. `a_lat = lat_g × lat_usage`, braking
  `brake_g × brake_usage`, both × the **road grip** the sim reports (`sync_unit_feel`: wet,
  cold, paint, aquaplaning via `wp_vehicle::Vehicle::conditions_grip`) × damage grip ×
  **confidence** (drops `confidence_drop` per slide or run-wide, floor `confidence_min`,
  regains `confidence_regain_per_s`). Shadowing (risk budget spent) keeps
  `shadow_cornering_g` as a cap; a pursuit or a response uses the limit. Obstacle and junction
  caps (`avoid`, e.g. `siren_junction_mps`) still apply on top.
- **Late braking**: the pedal command adds the profile's own deceleration a few metres ahead
  (feed-forward), read `lead_s` ahead in time.
- **Slide control** (above 5 m/s; body slip less the rolling slip `½·wheelbase·yaw/v`, which
  is large in a tight turn at a crawl): slip past `slip_deadband_rad` is countersteered
  (`countersteer_gain`, front wheels toward the velocity); past `slide_slip_rad` the driver
  lifts to `slide_throttle`. On the way out of a corner the throttle is capped by the friction
  ellipse from the measured lateral load (`exit_throttle_min` at full load), so the rear stays
  planted. Running `run_wide_m` outside the line in a corner lifts too.
- **Recovery**: facing away from the lane ahead → turn round at `turn_around_mps`; stuck
  (throttle on, not moving for `stuck_s`) → reverse at opposite lock until the nose points
  at the lane (`reverse_min_s`..`reverse_s`).
- **Car limits**: `set_unit_limits(unit, CarLimits)` (`wp_sim::Sim::set_police_car_limits`);
  default `[pro] default_limits` = the Sentinel Cruiser measured with `wp_vehicle::metrics`.

Measured (`cargo test --release -p wp_opponent --test police_pace -- --nocapture`; Sentinel
Cruiser, same car and measured capability for both, best race-AI profile = skill 1, both in
their own-direction lanes, full `wp_sim` loop):

| Course | Race AI | Police (pro) | Wall hits | Max body slip |
|---|---|---|---|---|
| Arterial square, best flying lap | 53.30 s | 52.73 s (−1.1 %) | 0 | 0.044 rad |
| Hollins Drive hill sprint (hairpins, 11 %) | 59.70 s | 61.08 s (+2.3 %) | 0 | 0.049 rad |
| Arterial square, fully wet (0.5 mm) | — | 59.22 s (+12.3 % vs dry) | 0 | 0.039 rad |
| Hollins Drive, fully wet | — | 71.78 s (+17.5 % vs dry) | 0 | 0.033 rad |

The pre-P8 driver (pure pursuit on the lane, corners planned at `pursuit_cornering_g` 0.65 g,
braking at 6.5 m/s², emulated in the same test): arterial square 60.51 s (+13.5 %), Hollins
Drive not finished (6 hits, stuck at a hairpin).

In the city with traffic (`cargo run --release -p simcli --example police_traffic_contacts --
pursuit 900`, 12×12 city at noon, now also counting police↔world hits), per 5 min of pursuit,
lights on: wall hits 5.7 (`main` before P8) → 2.1; traffic contacts 0.3 → 2.4, of which 7 of 8
are one low-speed stand-off (0.3–0.6 m/s, a held unit and a traffic car at a junction) counted
every few seconds. Patrol (lights off) is unchanged by construction (`patrol 300`: 3 wall hits,
as before).

### Obstacle awareness (`avoid.rs`)
Every tick each ground unit (physical or headless) looks ahead on its own track:
- **Lane corridor**: any vehicle whose box overlaps the unit's lane (unit half width + its
  projected half extent + `lateral_margin_m`) within a speed-dependent horizon
  (`min_horizon_m`..`max_horizon_m`). The nearest one caps the speed so the unit can stop
  `standstill_gap_m` behind it (braking at `decel_mps2` after `reaction_s`); a moving leader is
  followed at its own speed. A **bumper box** (`bumper_m` straight ahead of the body) catches
  what the corridor misses while the body is off its track (turning, cutting corners).
- **Hold**: at a zero cap, physical units get full brake (the shared controller would otherwise
  release it below its hold speed and creep).
- **Passing**: behind a *stopped* vehicle closer than `pass_gap_m` for `pass_wait_s` (6 s;
  `pass_wait_siren_s` 1.5 s with lights), the unit pulls `pass_shift_m` left (or
  `pass_extra_shift_m` further, for a car straddling the centreline) when that lane is clear past
  the blocker and free of oncoming vehicles for `oncoming_clear_m`. It passes at ≤
  `pass_speed_mps` and returns to its lane after the blocker.
- **Exceptions**: a unit in a `Pit` or `BoxIn` tactic ignores its own target (contact is the point).
  No unit ever overtakes its own target (a stopped target is an arrest).
- **Traffic rules** (next junction on the track, refreshed at think rate; light from
  `WorldQuery::signal_state_ahead`):
  - *No lights (patrol)*: stop `stop_line_back_m` before a red, and before a yellow when there is
    room; slow to `yield_mps` through unsignalised junctions and **give way** to cross traffic that
    gets there within `yield_gap_s` of the unit (or is in the junction box), waiting at most
    `yield_max_s`; turns at `turn_mps`; no overtaking within `no_pass_near_junction_m` of a
    junction (red-light queues).
  - *Lights on*: may run reds and exceed limits, but crosses junctions at `siren_junction_mps`
    (`siren_red_mps` against a red, giving way to cross traffic for up to `yield_max_siren_s`) and
    turns at `siren_turn_mps`.
- **Holding still**: at a zero cap, and when parked (tool units, no track), a physical unit holds
  brake 0.4 plus the handbrake. Full pedal at a standstill would engage `wp_vehicle`'s
  automatic reverse.
- **Gridlock**: a unit stopped behind another stopped police unit for `gridlock_s` creeps on at
  `pass_creep_mps` past stopped higher-id units that cross or face it (they keep waiting).
- **Spawning**: pool units appear in the right-hand lane, ≥ `spawn_clear_m` from every vehicle
  and outside every moving vehicle's next `spawn_path_s` of travel, already facing their first
  patrol route (a physical car turning around from standstill runs over the kerb). Reinforcement
  nodes use the same clearance.

Obstacles: `wp_sim` reports every vehicle (players, racers, physical and kinematic traffic,
police cars) with `set_obstacles` each tick. Headless, units see the racers and each other.

- **Push-through** (row 61, physical units): with lights on, blocked by a stopped civilian or
  parked car (not the racer, not a unit) for `push_after_s` (1.5 s) with no way past, a unit
  shoves it at `push_mps` (3.5 m/s ≈ 13 km/h) aiming a little left, for up to `push_max_s`. A
  push that moved it less than `push_min_progress_m` falls back to the un-sticking steps
  (back out, re-route, respawn). Push contacts are plain low-speed contacts: no heat for the
  racer, no crash scene below 8 m/s. `PoliceStats::pushes` counts them.
- Chasing units squeeze past a car that only clips the lane edge (`chase_lateral_margin_m`) and,
  farther than 150 m from every racer, drive through kinematic traffic (it has no collider there;
  near a player it turns physical).
- **Static feelers** (`walls.rs`, row 66, physical units): three rays from the front bumper
  (ahead and ±`wall_feeler_rad`) against building footprints and tunnel walls, as long as the
  stopping distance + `wall_margin_m`. A wall ahead caps the speed to stop 2.5 m short of it
  (`wall_decel_mps2`); a wall closing in on one side shifts the line `wall_shift_m` away from it.
  `PoliceStats::static_hits` logs every contact episode with static geometry (walls, parked
  props, kerbs) for the measurements.

### Tools & gadgets (`tools.rs`)
- `plan_site` puts a tool on the predicted path between `min_ahead_m` and `max_ahead_m`, never
  within 15 m of an intersection, and with approach sight distance ≥ `min_sight_m` (120 m).
  Sight distance is the minimum over **every** legal approach, so a site just after a turn-in
  point is a blind corner.
- **Roadblock**: 2–3 parked units plus barrier `StaticCollider`s with mass. Never on a highway
  when the level is above `highway_max_level`.
- **Spike strip**: a line collider. Wheels that cross it deflate linearly over `deflate_s` (5 s)
  → `tire_puncture(entity)` for `HandlingEffects::tire_puncture`.
- **Radar detector** `radar(pos, tier)`: units in Patrol/Pursue state within `radar_range_m[tier]`.
- **Scanner** `scanner(tier, since: Option<SimTime>)`: radio lines delayed by `scanner_delay_s[tier]` (10/6/3 s).

### Captain (`captain.rs`)
`CaptainCommand` (assign unit, roadblock, spikes, helicopter, priority target, call off) is
validated against unlocks (`presence.toml [unlocks]`), cooldowns, budgets and the placement
rules, and returns `CaptainError` on refusal. Use `to_bytes`/`from_bytes` for the payload of
`ClientToServer::CaptainCommand`. `captain_view()` returns units, incidents, heat by district,
racers' last-known positions with staleness, tools and readiness. Without a human captain
(`set_human_captain(false)`), the dispatcher places tools and requests the helicopter itself.
Player chase units get `unit_order()` **suggestions** only.

## Tuning tables (`data/police/`)
| File | Contents |
|---|---|
| `heat.toml` | Level thresholds; violation weights; witness, recognition, decay and escape tables per phase; capture, safehouse and bounty rules |
| `presence.toml` | Patrols per district × phase, reinforcement response times, reckless-driving response (`[response]`: call delays, radius, spawn window, unit cap by heat, despawn distance), helicopter availability, tool/tactic unlock levels, crash scenes |
| `pursuit.toml` | Think rate, speed factors and caps, cornering g, LOS range, unit dynamics and crash severity per mode, risk budget, traffic hazards, tactic parameters |
| `tools.toml` | Roadblock, spike strip and helicopter parameters, radar/scanner tiers, captain hold time |

The tables are compiled in (`PoliceConfig::builtin()`) and can be loaded from disk with
`PoliceConfig::load(dir)`. A test checks that both copies match.

## Roads
Roads come from the contract road network (`wp_contracts::RoadNetworkQuery`, v0.3.0; D's
`wp_world::RoadNetworkIndex`). Build police with `PoliceSystem::for_world(cfg, &world, setup)`, or
with `RoadGraph::from_network(&dyn RoadNetworkQuery, &signalised_nodes)` / `from_city(&CityMap)`
(`city.rs`). `RoadGraph` is the police routing index over that network, not a second road model.
The mapping is 1:1, with one segment (or a chain of them on curved roads) per edge:
- **Ids:** node ids are kept. `RoadSegment::edge` remembers D's edge id, and captains name roads
  by it (`RoadEdgeId`).
- **Classes:** Collector/Local/Alley → Local.
- **Districts:** Core/Commercial → Core, Residential/Suburb → Suburb, Industrial/Port → Industrial.
- **Line of sight:** `WorldQuery::line_of_sight`, which uses D's building footprints.

`RoadGraph` is only the police index on top: legal routing, predicted paths, approach sight
distance and nearest road. Tests, scenarios and tools use `city::procedural(&ProcGenConfig)`.

## Measured behaviour (`crates/wp_police/scenarios`, D's `ProcGenConfig::small()` city)
| Scenario | Result |
|---|---|
| Patrol presence (4 phases) | Exact table counts after warm-up |
| Day speeder, Core, 2× limit (5 runs) | Pursuit at 8.8 s mean (≤ 12.8 s every run); reinforcement 29 s |
| Night speeder, 03:00 (8 runs) | Pursuit at 51 s mean (126 s before round 12b's responders); reinforcement 102 s |
| Reckless-driving call → first unit within 100 m, racer at 2× limit (`response_*.ron`, 8 runs each) | Core noon 3.2 s mean / 7.6 s worst; Suburb noon 9.3 / 16.8 s; Core 23:00 7.1 / 17.6 s; Suburb 23:00 19.3 s mean, 6 of 8 within 25 s (one run: the racer loops the city edge and U-turns) |
| Pop-in audit, every scenario | 0 of 105 mid-session spawns and 0 of 333 despawns in a racer's view cone / sight or within 120 m |
| Cop self-wreck, Simulation (20 pursuits) | 0% of engaged units; careful units spend their risk budget (809 unit-seconds held to shadowing), an unlimited budget never does |
| Tool placement (5 runs, L4) | 35 tools, minimum sight 120.1 m |
| Escape (dusk, below helicopter level) | 2 of 3 runs escape |
| Capture / safehouse (12×12 city) / total loss | Each rule fires in its scripted case |
| Sideswipe a paused patrol car at 30 / 50 km/h, noon / 23:00 / 03:00 (3 runs each) | Assault every run, pursuit in the same tick (0.01 s), wanted L2 |
| 5 km/h parking bump (3 runs) | No assault, no heat, no pursuit |

| Response in the full sim, physical cars (`cargo run --release -p simcli --example police_response -- vesper\|grid <hour> 4 60`) | Port Vesper noon 13.3 s mean / 17.2 s worst, 23:00 28.2 / 34.0 s; grid noon 16.2 s, 23:00 12.8 s; 0 of 15 spawns and 0 of 6 despawns visible to the player (eye or chase camera) |
| City traffic, physical cars (`cargo run --release -p simcli --example police_traffic_contacts -- patrol\|pursuit 300`) | Patrol 300 s: 0 police↔traffic contacts, 1 overlap with kinematic traffic caused by a moving unit. Hot pursuit 362 s: 0 contacts, 0 overlaps with lights on, 3 caused by moving units in total |

## Perf
`cargo run --release -p wp_police --example police_bench`: 13 patrols + reinforcements, 4 wanted
racers on D's small city, with exact building line of sight and obstacle awareness → **0.097 ms per 60 Hz frame** (round 13: up to 8 units per racer, backup and search; 0.055 ms in round 12b, 0.074 ms in round 6)
(native, release, cloud Xeon 2.1 GHz). Budget 0.5 ms native / 1.0 ms browser. Browser not
measured yet.

Pro driver (physical units with lights on): 2.2–3.1 µs per physics tick per unit including
the line re-plans (`police_pace` test, release, same machine) → ≈ 12 µs per unit per 60 Hz
frame, ≈ 0.1 ms for 8 chasing units.

## Determinism
Absolute time is integer `SimTime` microseconds (`src/time.rs`); durations are `f32` seconds.
The crate uses no `f64`, no `HashMap` iteration and no libm transcendentals (only `wp_math::sin_cos`
and repeated multiplication). The RNG is seeded via `PoliceSetup::seed`. `lint-determinism` reports
no errors for this crate and needs no allow-list entries.

## Tests
```sh
cargo test -p wp_police                          # unit + system tests
cargo test -p wp_police --test police_scenarios  # scenarios, ratios, determinism
cargo run --release -p wp_police --bin police-scenario -- crates/wp_police/scenarios/day_core_speeder.ron
cargo run --release -p wp_police --example police_scenario -- <file.ron> [--tables <dir>]
```

## Known gaps
- The police graph ignores per-lane turn restrictions. Cameras come from the network's POIs (D
  places one at every third signalised junction). `RoadNetworkQuery` has no signalised-junction
  flag yet, so `from_network` takes the list separately (contract request #7).
- The headless kinematic body and its traffic-hazard model stand in for physical cars and real traffic.
- Passing assumes the other lane exists. Feelers see buildings and tunnel walls, not rails,
  barriers or terrain steps (not in the road network data).
- Pro driving (P8): the racing line only sees vehicles within `traffic_clear_m` ahead; a car
  alongside in the next own-direction lane is not in the line's room. Junction caps
  (`siren_junction_mps` 11 m/s) still hold pursuits back at every junction. Turning round in a
  junction is a full-lock crawl, which can touch a kerb on narrow roads.
- `PoliceSystem::new()` (used by `wp_sim` today) has no road network, so it tracks heat but spawns no units.
- Scripted racers don't react to PIT or box-ins; physics resolves those in the real game. PITs
  and rams need the lead unit within 35 m of a racer it can see on a fairly straight road; a
  racer at 90 km/h on Port Vesper's curvy roads rarely gives that (see the integration doc).
- Assault is not judged by fault: a moving racer that a patrol car drives into (rare with
  obstacle awareness) also counts. Overflow kinematic units in `wp_sim` (beyond the physical
  slots) can't be hit at all.
- Captain-command events are emitted on the next `step`.
- Never-in-view covers patrol refills, responders, reinforcements and despawns. Roadblock and
  spike units appear where the tool rules put them (≥ 300 m ahead with ≥ 120 m approach sight),
  so the racer can see them appear; a helicopter vanishes 15 s after going out of service.
- Racers' sight is checked at their own height for both ends: on a steep hill a spot below a
  crest may count as visible (conservative) or a spot above as hidden.
- No JSON Schema for `data/police/*.toml`; validation lives in `PoliceConfig::validate` plus serde `deny_unknown_fields`.
