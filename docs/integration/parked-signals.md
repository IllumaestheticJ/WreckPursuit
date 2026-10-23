# Parked-car damage + traffic turn signals (lead helper `lead/parked-signals`)

- **Workstream / prompt:** lead helper — owner playtest feedback 2026-10-21: "Parked cars don't
  take damage" and "Traffic doesn't use turn signals".
- **Branch:** `lead/parked-signals` (from `lead/round12`)
- **Crates / folders touched:** `crates/wp_sim` (new `parked.rs`, `props.rs`, `contacts.rs`,
  `lib.rs`, example `parked_damage_cost`), `apps/simcli` (`parked` field, `parked_*` world
  metrics, `scenarios/parked/*`, goldens), `crates/wp_wasm` (`props.rs`, `lib.rs`,
  `session.rs`, `lod.rs`), `apps/testbed-web` (`render/props.ts`, `render/panels.ts`,
  `wasm/*`, `e2e/run.mjs`), and for signals `crates/wp_world` (`traffic.rs`, new
  `traffic/signal_tests.rs`, `tests/siren.rs`, README) and `apps/testbed-web` (`render/fleet.ts`,
  `render/lod.ts`, `vehicle_view.ts`, `net/main.ts`, new `indicator_stalk.ts`, `e2e/signals.mjs`)
- **Contract version built against:** 0.3.0 (no contract change; one request below)

## 1. Parked cars dent

### The bug
Parked cars are `wp_world` props (`PropKind::ParkedCar`, laid out from `data/city/parking.json`)
that `wp_sim` turns into Rapier boxes: fixed until a mover comes near, then dynamic, then fixed
again once asleep (`wp_sim/src/props.rs`). They could be shoved but had no damage model: a
50 km/h hit left a pristine box.

### What changed (`crates/wp_sim/src/parked.rs`)
- **Promotion on the first real touch.** After the physics step, every *dynamic* parked car
  (only those can be touched; fixed ones have no mover near) gathers its contacts. One that took
  ≥ 50 N·s (`PROMOTE_IMPULSE_NS`) from a moving body at ≥ 1 m/s closing speed
  (`PROMOTE_CLOSING_MPS`, the lattice's own `min_contact_speed_mps`) in one tick gets a
  `wp_damage::DamageModel` of the session's parked-car model (`Sim::set_parked_car`; simcli and
  the testbed use the traffic car, `debug_sedan`, which is also what the testbed draws parked
  cars as). The impact's first tick already reaches the lattice. Pushes, resting contacts and
  parking-speed nudges never promote.
- **Same lattice, same tables.** The wreck's contacts (from its own point of view, via the
  shared `contacts::body_contacts`, which `contacts::collect` now also uses) are mapped into the
  car's chassis frame (`ChassisMap`: the lattice's rest bounds scaled onto the 1.9 × 4.5 × 1.5 m
  prop box, scale ≈ 0.96–1.04) and applied with the sim's `ModeTable`. Arcade vs Simulation is
  only the table, exactly as for driven cars (owner rule 2026-10-20); nothing else branches.
- **Collider follows the crush**, like a driven car's hull: rebuilt from
  `convex_hull_outer()` on a flat floor (the box's four bottom corners, dented points kept a
  sill height above it). A first version projected every hull point onto the floor; the sliver
  bottom face made the car rock forever on the slab and keep re-waking its lattice (found by a
  sleep test, fixed before commit).
- **Loose parts**: `take_detached_parts()` → debris bodies (`DetachedPart::to_rapier` at the
  chassis-origin pose), drawn by the existing debris path. **Glass**: shattered regions hidden by
  the renderer (see below).
- **Sleeps again.** The lattice is fed only on ticks it has a contact it would take (≥ 1 m/s) or
  while it is still moving; asleep it costs nothing. The prop body goes back to fixed as before
  and keeps its dents.
- **Chains.** A prop shoved faster than 1 m/s now wakes the props around it like a vehicle
  (`props.rs`), so a side-swiped parked car pushed into the next one meets a dynamic body (that
  can dent too) instead of an immovable fixed box.
- **Cap**: `MAX_PARKED_WRECKS` = 48 promoted per session (≈ 0.3 MB each); past that, hit parked
  cars are shoved boxes as before (`ParkedStats::over_cap` counts them).
- **Events.** A parked car's damage events (glass, parts, components, total loss) are *not* put
  in the sim event stream: police (`TotalLoss`), economy, race and the net server key events by
  vehicle entity. They are counted in `Sim::parked_stats()`; renderers read the lattice.
- **Hash / determinism.** Wrecks are kept in prop order and promoted in prop order from sim state
  only. `Sim::state_hash` adds them (count, prop, lattice hash, body position) only once one
  exists, so every run without a dented parked car keeps its hash: **all 109 existing goldens are
  unchanged.** `Sim: Clone` copies them (crash replays).

New `wp_sim` API (lead-owned crate): `Sim::{add_parked_car, set_parked_car, parked_wrecks,
parked_wreck, parked_stats, prop_kind, prop_half_extents}`, `ParkedWreck`, `ParkedStats`,
`ChassisMap`, `prop_of_entity`, constants `PARKED_ENTITY_BASE` (0x4000_0000), `MAX_PARKED_WRECKS`,
`PROMOTE_IMPULSE_NS`, `PROMOTE_CLOSING_MPS`, `PARKED_CAR_HALF_EXTENTS`, `PARKED_CAR_MASS_KG`.
`Sim::node_positions(PARKED_ENTITY_BASE + prop)` returns a wreck's lattice.

### Measured (debug sedan, flat world)
| Hit | Mode | Parked car: structural crush / health | Notes |
|---|---|---|---|
| 50 km/h into its tail (unit test, any-node crush) | Arcade | 0.235 m / 0.99 | a glass panel broken, shoved 4.1 m |
| same | Simulation | 0.372 m / 0.89 | glass broken, a part torn off |
| 50 km/h rear hit, parked vs the same car standing on its wheels (parity test) | Arcade | 13.1 kJ plastic / 0.046 m vs 13.2 kJ / 0.046 m | "dents like any car" |
| `parked_frontal_50_arcade` | Arcade | 0.028 m / 0.987 | shoved 5.0 m; striker 0.067 m / 0.985 |
| `parked_frontal_50_sim` | Simulation | 0.126 m / 0.871 | shoved 5.0 m; striker 0.141 m / 0.865 |
| `parked_swipe_row_50_arcade` | Arcade | 2 dented, 0.154 m / 0.96 | striker spins out |
| `parked_swipe_row_50_sim` | Simulation | 1 dented, 0.314 m / 0.787, a part off | striker 0.556 m / 0.80 |

### Scenarios (`apps/simcli/scenarios/parked/`, goldens blessed)
`frontal_50_{arcade,sim}` (50 km/h into a parked car's tail, then brake) and
`swipe_row_50_{arcade,sim}` (50 km/h side-swipe of a row of four parked cars, 0.2 m overlap).
Expectations are ranges on the new world metrics. New scenario field `parked: [(pos, yaw)]`
(`Sim::add_parked_car`, any world) and world metrics `parked_dented`, `parked_crush_max_m`,
`parked_health_min`, `parked_glass_shattered`, `parked_parts_detached`, `parked_moved_max_m`
(`apps/simcli/SCENARIOS.md`). simcli sets the parked model whenever the sim has props.

### Testbed (`apps/testbed-web/src/render/props.ts`)
A dented parked car (`parked_wrecks()`, new wasm call, one small array per frame) is drawn like a
driven car: its own `PanelBody` (the per-panel mesh R built for driven cars) skinned from its
lattice (`lattice_nodes(entity)`, re-read only while its lattice is awake), wheels at ride
height, inside a group carrying the prop pose and the chassis→box map, shattered glass hidden
(`parked_wreck_shattered`). Its instance (full / proxy / box tier) is hidden once the panel mesh
is loaded, so it is always drawn exactly once. `PropView.wreck(i)` and `stats().wrecks /
wrecksDrawn / wreckReskins` for automation. wasm: a wreck's entity id
(`parked_entity_base() + prop`; too big for a Float32Array, so not in the list) works with
`vehicle_info` (kind `"Parked"`), `lattice_*` and `panel_states` (empty).
`props_static()` now reads kinds and sizes from the sim (scenario `parked` cars are drawn too).

e2e (`e2e/run.mjs`, "Props"): the existing 43 km/h knock in the city chase must now also dent
the car (health < 1) and draw the dent (own panel mesh loaded, largest drawn vertex offset
> 5 mm). Result (cloud container, headless Chromium, city chase): parked car #810 knocked at
43 km/h → health 0.923, own panel mesh drawn, largest drawn vertex offset 0.189 m, drawn vs sim
pose ≤ 0.000 m every frame; 2,126 parked cars still each drawn exactly once; `parked_wrecks()`
< 0.001 ms. Whole `run.mjs` main section: 46/46 PASS.

## 2. Traffic turn signals

### What existed
`wp_world` already computed `AgentView::turn_signal` (and the contract's
`TrafficAgentSnapshot` LEFT/RIGHT/HAZARDS flags reached the browser and the net), but: the
junction signal came on at a flat 40 m and went **off while turning** (on the connector), and
**the testbed never drew it** (only hazards tinted the tail lamps). That is what the owner saw.

### What changed (`crates/wp_world/src/traffic.rs`, view-only)
- **Junction turns**: on at `turn_signal_lead_m(v)` = clamp(v × 3 s, 30 m, 50 m) before the lane
  end, latched (a car slowing into a queue keeps it on), held **through the connector** (left,
  right, U-turn as left), off on the exit lane.
- **Lane changes**: on while pending and through the whole lateral move, off within 0.3 m of the
  new lane centre (≈ 3 s in total). Signalling 1.5 s *before* the move (instead of the existing
  0.5 s) was tried and reverted: it changed traffic behaviour (146 → 96 lane changes in
  `traffic_bench`) and every traffic golden.
- **Pulling over for sirens** (towards the kerb), waiting, **merging back** (away): as before,
  now tested. **Hazards**: disabled / crashed cars (hazards only), signal-less manoeuvres.
- No parking / pull-away from the kerb exists in traffic, so nothing to signal there.
- The signal is view state: not in the state hash, never read by the driving logic.
  `traffic_bench` hash identical before/after; **no golden moved.**
- Tests: `traffic/signal_tests.rs` (lead distance; 71 right turns tracked on a signalised grid:
  off at 80 m, onset 30–50 m and ≥ 2 s out, held in the connector, off on the exit lane;
  lane changes signalled ≥ 0.5 s before and during the move), `tests/siren.rs`
  `pulling_over_and_merging_back_are_indicated`.

### Testbed
Amber indicator lamps at the four corners (below head/tail lamps, wrapping the corner),
blinking 1.5 Hz at 50 %, `PALETTE.hazard` × the brake glow so they bloom at night and read in
daylight; hazards blink both sides. Drawn in: the instanced fleet (kinematic + far traffic; +2
draw calls per traffic instance set), a promoted car's `VehicleView` (flags from its traffic
draw record), and the net client's traffic boxes (`net/main.ts`, from the snapshot's contract
flags). Player: a cosmetic stalk (`indicator_stalk.ts`, keys `-` left / `=` right, self-cancels
after the wheel returns past a turn) — not in the sim, replay or net (no `VehicleInput` field;
see the contract request). Police AI cannot indicate for the same reason.
e2e `npm run e2e:signals` (`e2e/signals.mjs`): 6 right- and 9 left-signalling cars blink the
correct side (lit and dark frames, full and proxy meshes), never the wrong side; hazards both
sides; the player stalk blinks and cancels; no page errors.

### Police cars indicate (ported, lead request)
From the unmerged `claude/happy-knuth-g39ogc` (commits `a5c0b5b` / `fcc9ea3`, older base) I took
only the turn-signal part: `wp_police::Unit::turn_signal()` (40 m before a junction where its
route turns until 8 m past it; left pulling out to pass a stopped car, right pulling back in; off
with lights and siren and for helicopters), `JunctionAhead::turn_dir` computed where the
junction ahead is found (`dispatch.rs`), its unit test, `wp_wasm::police::turn_signal` and
`vehicle_meta()` bits 16 / 32, fed to the police car's `VehicleView.setIndicators` in
`main.ts`. View only (police state hash untouched). Not taken: the rest of that commit
(stuck-cop behaviour fixes, Loop 9, blessed goldens: separate work on an older base) and its
own lamp meshes (we have the helper's `VehicleView` lamps).

## 3. Parked cars in the ground (owner bug on preview 0.12.26, "Hollins Drive")

**Reproduced** (`crates/wp_sim/tests/parked_ground.rs`: the static surface under each parked
car's four wheel points and four body corners, corners a 0.15 m sill above the wheels): on
Port Vesper v2, **671 of 2,026 parked cars sat more than 3 cm inside the surface** (worst
0.76 m), **every one a driveway car** on a residential lot (`wp_world::residential::driveway_cars`):
94 of the 176 parked cars around Hollins Heights. No kerbside car sank; the procedural grid
had none. Cause: props were seated on `CityMap::ground_height` (the logical lot / road height)
and a slope sampled 1 m around, not on the solid surface the browser draws (terrain and
road / sidewalk meshes, "solid terrain = drawn terrain"), which on hillside lots is higher.

**Fix (`wp_sim`, cities with terrain):** `Sim::set_world` seats every parked car from raycasts
on the drivable ground colliders (road / sidewalk trimeshes, terrain heightfield:
`PhysicsWorld::drivable_ground_z`) under its four wheels: pitch and roll fitted to them,
lifted so no wheel is below the surface, 2 cm hover (so it sleeps). Spots where a car cannot
stand are dropped: steeper than 0.15 along / 0.10 across, wheels more than 6 cm off one plane
(straddling a kerb), a body corner in the ground, or something solid (tree trunk, hedge,
fence, wall) in the footprint. **158 of 2,026 spots dropped** (1,868 parked cars remain,
1,075 of them on driveways); `World::set_parked_cars` (one small setter in `wp_world`) keeps
the world's list equal to the sim's, so police obstacles and resets see the same cars. The
drawn car already follows the collider pose (`props_static()`), so drawn = solid.

**Knocked / dented cars:** a resting prop on a hilly city ignores the terrain until it wakes;
it used to get terrain contacts back only once faster than 0.5 m/s *after a 1 s settle
window*, so a car knocked in the first second (or pushed slowly) was shoved 0.23–0.38 m into
the hillside (reproduced near Hollins, also with dents off: a prop bug, not the lattice). Now
it collides with terrain as soon as it turns dynamic (a mover is near; only the few active
props). Dented cars keep the box's flat floor, so promotion never lowers them.

**Test** (`parked_ground.rs`, 2.7 s debug): no parked car more than 3 cm in the surface in
Port Vesper (worst now 1.7 cm *above*) and the grid (0), and the world keeps exactly the sim's
cars; and after knocking six parked cars around Hollins Heights at 40 km/h (5 dented) and
letting them settle, none is (deepest 1 mm). Goldens: `race_lantern_checkpoint` and
`world_traffic_soak_vesper` re-blessed (the Vesper runs that meet parked cars; expectations
all pass); the other 111 unchanged.

## Prior prototypes (lead request)
- `claude/optimistic-wozniak-13v80v` `1617233` "prototype dentable parked cars": swaps a parked
  prop about to be hit for a full driverless `wp_sim` vehicle (Rapier chassis, wheels, vehicle
  sim, at most 8, swapped back once at rest). **Not merged**: the task asks not to pay a full
  vehicle sim per parked car, and this branch's lattice-only promotion already covers it (48
  at once, no wheels / drivetrain, promotion only on a real hit instead of on approach). Idea
  kept from it: a dented car keeps its damage when it turns back into a sleeping prop (here
  the wreck is never dropped).

## Perf numbers
Native, release, cloud container (4 vCPU, load ≈ 2), `cargo run --release -p wp_sim --example
parked_damage_cost [arcade|sim]`: 24 debug sedans at 50 km/h, each into its own parked car and
clipping the next (48 parked, 24 dented in the same second, the worst case), per 60 Hz frame
(4 ticks), dents on vs off (off = plain shoved boxes, as before):

| Measurement | Arcade | Simulation | Budget |
|---|---|---|---|
| 24 cars + 24 fresh dents, mean frame | 12.4 ms (off 6.6) | 15.0 ms (off 7.0) | — (crash spike) |
| worst frame | 79 ms (off 38) | 80 ms (off 39) | — |
| one awake dented parked lattice | ≈ 0.73 ms/frame for ≈ 1.3 s | ≈ 0.99 ms/frame | same as a driven car resolving a crash (`wp_damage` README: 1.0 ms/frame); budget 0.25 ms/car/frame is the amortised figure |
| asleep dented parked car | 0 (not stepped) | 0 | — |
| 24 cars driving past 408 parked cars, none hit | 1.98 ms (off 2.43 / 2.18) | 1.72 ms (off 1.64 / 1.72) | within run-to-run noise |
| traffic turn signals (`traffic_bench`, 400 agents) | 0.238 ms/frame (before 0.228) | — | 2 ms |

So dents cost nothing until a parked car is hit, then exactly what a driven car's lattice
costs while it resolves the crash, and nothing again once it settles. A first version cloned the
whole parked car model every tick (+0.3 ms/frame with 400 woken parked cars); fixed before merge.

Browser: `PropView.update()` 0.185 ms mean / 3.5 ms worst frame (the worst is the frame a
dented car's panel mesh is built); `parked_wrecks()` < 0.001 ms. Indicator lamps: +2 draw calls
per traffic instance set.

`e2e/perf_check.mjs` on this container (software GL, 3–4 fps) fails 10 checks; **`origin/lead/round12`
fails 9 of the same 10 on the same container** (real-time factor, frames over 250 ms,
allocations, phone draw calls). The extra one is the phone real-time factor at 0.892 vs base 0.902
(noise level). Differences vs base: phone 348 vs 338 draw calls (indicator lamps; base already
over the 300 limit), allocations chase 794 vs 847, rain 819 vs 835, phone 821 vs 700 KB/frame.
`e2e/reset.mjs` failed once under load (ghost blink) and passes on a quiet machine.

## Contract change requests
| # | Change | Why | Who is affected | Status |
|---|---|---|---|---|
| 1 | `VehicleInput::signal: i8` (or a `lights: u8` bit field: left, right, hazards) | Player and police cars cannot indicate through the sim, the replay or the net today: `VehicleInput` has no field, only `TrafficAgentSnapshot.flags` (traffic) does. With it: police AI could indicate when pulling over a suspect, the player's stalk would reach other clients, and `wp_net` snapshots could carry it like brake lights. | contracts, wp_vehicle (pass-through), wp_net (snapshot flags), testbed (VehicleView lamps), simcli replay (input op) | proposed |

## Changes outside my crates
- `crates/wp_world` (owner D): `World::set_parked_cars` (replace the parked list after the sim
  seated them; used only by `Sim::set_world` on cities with terrain).
- `apps/simcli` (owner 10): `Scenario::parked` + `ParkedSpec`, `PARKED_CAR`, `parked_*` world
  metrics, `SCENARIOS.md`, `scenarios/parked/*`, goldens; `parked: Vec::new()` in `fuzz.rs` and
  `examples/city_frame_cost.rs`.
- `crates/wp_wasm` / `apps/testbed-web` (owner 04): parked-wreck calls and drawing as above;
  `PanelBody::maxDisp()` (tests); `SessionModels.parked`.
- Signals: `crates/wp_world` (owner D) traffic view state + tests; testbed files as listed above.

## Known gaps
- A parked car's damage makes no sound and no crash FX (its events are kept out of the stream);
  wiring `GlassShattered` / `PartDetached` with a parked id into `wp_audio` and the fx needs
  their consumers to accept non-vehicle ids (J, C).
- Parked cars use one model (the traffic car) for both the lattice and the drawn mesh; the
  lattice is scaled ±4 % onto the prop box. Per-prop models would need per-prop car ids in
  `wp_world` (D).
- Wheels of a dented parked car stay drawn at ride height even if its wheel tore off (the wheel
  debris is drawn separately); no hinged panels (`panels: true` only applies to scenario cars).
- Multiplayer has no parked cars (`parked_cars: false` in `wp_net`); if F turns them on, the
  snapshot needs wreck lattices (like vehicles' `DamageSnapshot`).
- More than 48 dented parked cars in one session: the rest are shoved without denting.
- Player and police signals are local only (cosmetic stalk; police none) until contract
  request #1. No gamepad binding for the stalk.
- The e2e did not catch a promoted traffic car (`VehicleView`) signalling; that path shares
  the flag → lamp code and has a read-back (`window.__wp.indicators`).
- Lane changes pre-signal 0.5 s (behaviour-preserving), not the 1.5–2 s a careful driver would;
  lengthening it changes traffic and its goldens (D's call).
