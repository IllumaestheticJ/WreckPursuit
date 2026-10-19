# Scenario format (`.ron`)

Scenarios live in `apps/simcli/scenarios/<group>/*.ron`. Groups: `smoke/` (quick sanity),
`regression/` (bug repros, multi-car), `golden/` (the "feel" library; these also get a golden
replay), plus any folder a workstream adds (`vehicle/`, `damage/`, `police/`, …).

Parsing uses RON with `implicit_some`, so optional fields are written plainly
(`entity: "car"`, not `entity: Some("car")`). Unknown fields are errors.

## Top level

```ron
(
    name: "golden_frontal_56kmh",   // unique; output folder out/<name>/, golden folder golden/<name>/
    seed: 103,                       // master RNG seed (u64)
    city: flat,                      // flat | procedural(seed, blocks_x, blocks_y, block_m) | file("city/x.json")
    time_of_day_h: 12.0,             // start time, hours        (default 12.0)
    time_ratio: 10.0,                // game s per sim s         (default 10.0)
    mode: Simulation,                // Arcade | Simulation      (default Arcade) → data/modes/{arcade,sim}.toml
    assists: ai,                     // default assists: keyboard | gamepad | wheel | ai | off | custom((abs: true, …))
    traffic_density: 0.3,            // optional override 0..=1
    police_presence: 1.0,            // optional override 0..=1
    systems: "vehicle,damage,world", // optional; default all. --systems overrides. "net" adds the snapshot round trip
    barriers: [(pos: (0.0, 45.0, 0.75), size: (8.0, 1.0, 1.5), yaw: 0.0)], // optional static boxes (Sim::add_static_box)
    entities: [ … ],                 // spawn order = entity id order (1, 2, 3, …)
    duration_s: 8.0,
    sample_hz: 60.0,                 // metrics.csv rate (default 60)
    hash_every: 24,                  // hashes.txt / replay checkpoint interval in ticks (default 240 = 1 s)
    expect: [ … ],                   // must pass
    expect_pending: [ … ],           // evaluated + reported, never fail (targets for stub systems)
)
```

Cities load only with `RunOptions::load_city` (the browser testbed sets it; the CLI does not
yet, so the city goldens keep their flat-world hashes until they are re-blessed). `file` cities
must be `.road.json`. Features a crate does not support yet (cities without `load_city`,
traffic/police overrides,
`follow_route`, `ai_driver`) run anyway and add a **warning** to the summary; `--strict` turns
warnings into failures.

## Entities

```ron
(
    id: "striker",                  // scenario-local name used by expectations
    car: "debug_coupe",             // data/cars/<car>.json
    pos: (-40.0, 0.0, 0.0),         // world m (default origin)
    yaw: -1.5708,                   // rad about +Z, 0 = facing +Y (default 0)
    assists: off,                   // optional per-entity override
    driver: cruise(target_mps: 13.9, from_s: 0.0, until_s: 5.0),
    inputs: [
        (t_s: 0.0, input: (throttle: 1.0)),
        (t_s: 2.5, input: (steer: 0.4, brake: 0.2, gear_up: true)),
    ],
)
```

- `inputs`: keyframes ascending by `t_s`; each `VehicleInput` holds until the next key
  (missing fields are 0/false). `gear_up`/`gear_down` are edges: they apply on the key's first
  tick only. A key at `t_s` applies before tick `round(t_s / PHYSICS_DT)` is stepped.
- `driver`:
  - `scripted` (default): keyframes only.
  - `cruise(target_mps, from_s, until_s)`: harness P-controller on throttle/brake (pedals
    quantised to 1/32, ±0.1 m/s deadband) between `from_s` and `until_s`; steer and the other
    fields still come from the keyframes. Use it to arrive at a crash at an exact speed.
  - `follow_route(route_id, target_speed)`: needs `wp_world` routes (prompt 05). Warns for now.
  - `ai_driver(target)`: needs the AI driver (prompts 05/06). Warns for now.

## Expectations (`expect`, `expect_pending`)

All ranges are inclusive. `entity` defaults to the first entity.

| expectation | checks |
|---|---|
| `metric(name, entity, min, max)` | any per-entity summary metric (list below) |
| `speed_at(entity, t_s, min, max)` | forward speed (m/s) right after tick `round(t_s/dt)` |
| `max_lateral_g(entity, min, max)` | max \|lateral accel\| in g over 1/60 s windows |
| `crush_depth(entity, min, max)` | max lattice node displacement from rest, m |
| `components_failed(entity, min, max)` | distinct components with a `ComponentFailed` event |
| `total_loss(entity, expected)` | a `TotalLoss` event was / was not emitted |
| `pursuit_started_before(t_s)` | a `PoliceEvent` with kind `simcli::PURSUIT_STARTED_KIND` before `t_s` |
| `police_event_before(kind, t_s)` | a `PoliceEvent` of `kind` before `t_s` |
| `event_count(event, entity, min, max)` | number of `SimEvent::<event>` (optionally naming `entity`) |

Metrics for `metric(...)`: `final_speed_mps`, `max_speed_mps`, `distance_m` (horizontal,
from spawn), `path_length_m`, `final_health`, `max_lateral_g`, `max_long_g`,
`max_yaw_rate_rps`, `crush_depth_m`, `components_failed`, `parts_detached`, `wheels_detached`,
`total_loss` (0/1), `min_z_m`.

Use ranges, never exact numbers: the hash goldens already pin exact behaviour, and ranges
describe what "correct" means so they survive tuning.

## Outputs (`simcli run`)

`out/<name>/`:

| file | contents |
|---|---|
| `metrics.csv` | `t_s,tick,entity,x,y,z,speed_mps,lat_g,long_g,yaw_rate_rps,engine_rpm,gear,health,crush_depth_m` at `sample_hz` |
| `events.jsonl` | one `{"tick", "t_s", "event": <SimEvent>}` per line |
| `hashes.txt` | `# header`, then `<tick> <16-hex state hash>` every `hash_every` ticks and at the end |
| `summary.json` | per-entity metrics, expectation results, pending results, warnings, failures |
| `replay.bin` | setup + input stream + per-tick hashes + checkpoints (`src/replay.rs` documents the layout) |

## Example: the smallest useful scenario

(`tests/scenarios.rs` parses and runs this block, so it stays correct.)

```ron
(
    name: "vehicle_brake_100_0",
    seed: 1,
    assists: ai,
    entities: [(id: "car", car: "debug_sedan", driver: cruise(target_mps: 27.8, until_s: 14.0),
                inputs: [(t_s: 14.0, input: (brake: 1.0)), (t_s: 17.0, input: ())])],
    duration_s: 18.0,
    expect: [speed_at(t_s: 13.9, min: 27.0, max: 28.6), speed_at(t_s: 17.0, min: 0.0, max: 2.0)],
)
```
