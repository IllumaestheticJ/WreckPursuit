# data/fleets — mixed police and traffic fleets

Used by scenarios with `fleets: true` (the browser free drive and city chase presets set it;
simcli scenarios default to off, so blessed goldens keep their single police / traffic model).

| File | What | Read by |
|---|---|---|
| `police.toml` | Which car each AI police ground unit drives: per model a patrol weight, reinforcement and roadblock-tool weights by wanted level L1–L5, and measured limits for the pro driver | `wp_police::PoliceFleet` → `wp_sim::Sim::set_police_fleet` |
| `traffic.toml` | Civilian models: length / width for spacing and overlap tests, spawn weights on city vs highway lanes, speed cap, acceleration and headway scaling | `wp_world::FleetFile` → `wp_sim::Sim::set_traffic_fleet` |

- **Police**: a unit's model is drawn when it gets its physical body, from its role (district
  patrol, reinforcement, roadblock / spike unit) and the wanted level of the most-wanted racer at
  that moment. The draw hashes the unit id with the file's `seed` (no shared RNG stream). Limits
  are `[skidpad g, 100–0 km/h m, 0–100 km/h s, top speed km/h]` from `data/cars/VALIDATION.md`.
- **Traffic**: the model is drawn at spawn from the lane's weights (semis, box trucks and the
  tipper mostly on highway lanes). Agents keep their model for life; promoted (near-player)
  agents become that car with full physics and damage. In the browser, kinematic agents are drawn
  as instanced copies of the model's panel mesh, painted from the car's own palette.
- Every `car` must exist in `data/cars/`; `cargo test -p simcli --test fleets` loads both files,
  runs the golden city chase with fleets on and checks variety and determinism.
