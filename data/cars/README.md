# data/cars — vehicle definitions

One `VehicleDef` JSON per car: `data/cars/<id>.json` (`id` must equal the file stem). Schema:
`schemas/vehicle_def.schema.json`. Load + cross-reference check: `wp_contracts::load_car`.
Roster work is owned by workstream 09; the debug cars belong to the integration lead.

## Naming policy (hard rule)

**No licensed names anywhere** — not in ids, `display_name`, `analog_of`, comments, spec sheets,
liveries or commit messages. That means no manufacturer names, brand names, model names, badges,
trim designations that are trademarks, or recognisable logos/liveries.

- Use an invented analog name: e.g. `"display_name": "Kestrel GT"` with
  `"analog_of": "late-90s Japanese FR coupe"`.
- `analog_of` describes the *archetype* only: era, origin region, layout, body style, engine
  type/size. Good: `"2010s American V8 muscle coupe, 5.0 L"`. Bad: anything naming a maker/model.
- If an invented name is too close to a real nameplate (sound-alike, one letter off), rename it.
- Workstream 09 adds a grep check against a list of real brands/nameplates; keep it passing.

## Field guide

All units SI. Chassis space: origin at the middle of the wheelbase on the ground, +X right,
+Y forward, +Z up. Wheel arrays are ordered **FL, FR, RL, RR** (the loader rejects other orders).

| Field | How to fill it |
|---|---|
| `class` | `Beater, Compact, Sport, Muscle, Exotic, Truck, PoliceCruiser, PoliceInterceptor, PoliceSuv` |
| `body_size_m` | `[width, length, height]` of the body, m |
| `mass_kg` | Curb mass + driver |
| `front_weight_frac` | Static front axle share (FF ≈ 0.60–0.63, FR ≈ 0.50–0.55, MR ≈ 0.40–0.44) |
| `wheelbase_m`, `track_front_m`, `track_rear_m` | Spec sheet |
| `cg_height_m` | ≈ 0.45–0.52 sedans, ≈ 0.40–0.46 sports/exotics, 0.65–0.75 trucks/SUVs |
| `inertia_kgm2` | `[roll, pitch, yaw]`; box approximation `m·(a²+b²)/12` scaled ≈ 0.8–1.0, show the derivation in the spec sheet |
| `drag_cd_a_m2` | Cd × frontal area (≈ 0.6–0.9 cars, 1.1–1.4 trucks) |
| `lift_front`, `lift_rear` | Cl·A, m² (positive = lift) |
| `engine.torque_curve_nm` | 10–15 `[rpm, Nm]` points, strictly increasing rpm |
| `engine.idle/redline/rev_limit_rpm` | `idle < redline ≤ rev_limit` |
| `engine.inertia_kgm2`, `engine_brake_nm` | ≈ 0.1–0.3 kg·m²; engine braking at redline |
| `drivetrain.layout` | `"Fwd"`, `"Rwd"`, or `{"Awd": {"front_split": 0.4}}` |
| `drivetrain.diff_front/rear` | `"Open"`, `"Locked"`, or `{"Lsd": {"preload_nm", "ramp_power", "ramp_coast"}}` |
| `gear_ratios`, `final_drive`, `reverse_ratio` | Spec sheet |
| `wheels[i].position_m` | Hub position; z = loaded radius |
| `wheels[i].tire_id` | A file in `data/tires/` |
| `suspension[i]` | Wheel-rate spring from ride frequency: `k = m_corner·(2πf)²` (f ≈ 1.1–1.6 Hz street, 1.8–2.2 sport); damping ζ ≈ 0.25–0.35 bump, ×1.5 rebound |
| `brakes` | Per-wheel max torque from rotor size × pad class; `bias_front` 0.6–0.7 typical |
| `steering.ratio` | 12–18 typical |
| `lattice_id` | A file in `data/lattices/` |
| `durability_class` | 1 (fragile) … 5 (battering ram) |

## Current files
- `debug_sedan.json` — 1400 kg FWD sedan, ≈150 kW (generic test car).
- `debug_coupe.json` — 1500 kg RWD coupe, ≈300 kW, clutch LSD (generic test car).
