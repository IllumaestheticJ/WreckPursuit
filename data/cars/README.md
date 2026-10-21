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

Debug cars (integration lead):
- `debug_sedan.json` — 1400 kg FWD sedan, ≈150 kW (generic test car).
- `debug_coupe.json` — 1500 kg RWD coupe, ≈300 kW, clutch LSD (generic test car).

Roster (workstream 09). Each car has `<id>.json`, a spec sheet `<id>.spec.md` (every
derivation written out) and a lattice `data/lattices/<id>.json`. All three are **generated**
by `tools/specsheet/build_roster.py` from `tools/specsheet/roster_inputs.py` — edit the
inputs, not the outputs. Overview table: `ROSTER.md`; validation: `VALIDATION.md`.

| Id | Name | Class | Archetype (short) |
|---|---|---|---|
| `vireo_xs` | Vireo XS | Compact | late-90s FF 1.6 NA hot hatch — beater-tier hero |
| `kestrel_ts` | Kestrel TS | Sport | mid-2000s FF 2.0 T sedan (AWD via mod) |
| `shiro_turbo` | Shiro Turbo | Sport | early-2000s FR 2.0 T coupe, LSD |
| `hakone_gt` | Hakone GT | Sport | 90s FR 3.0 TT grand tourer |
| `rampart_50` | Rampart 5.0 | Muscle | 2010s 5.0 V8 muscle coupe |
| `ironside_440` | Ironside 440 | Muscle | early-70s 7.2 L big-block, live axle, scary brakes |
| `sperber_32` | Sperber 32 | Sport | 2000s FR 3.2 I6 sport sedan |
| `lindwurm_4t` | Lindwurm 4T | Sport | 2010s AWD 2.0 T hot hatch, dual-clutch |
| `lucciola_350` | Lucciola 350 | Exotic | 90s mid-engine 3.5 V8 |
| `hauler_1500` | Hauler 1500 | Truck | 2010s half-ton 5.3 V8 pickup, ladder frame |
| `sentinel_cruiser` | Sentinel Cruiser | PoliceCruiser | RWD 5.7 V8 police sedan (pursuit package), push bar |
| `sentinel_wolfhound` | Sentinel Wolfhound | PoliceInterceptor | AWD 3.5 TT V6 pursuit sedan, push bar |
| `sentinel_bastion` | Sentinel Bastion | PoliceSuv | RWD 5.3 V8 police SUV, ladder frame, push bar |
| `junkyard_beater` | Junkyard Beater | Beater | late-80s FF 1.6 SOHC sedan, worn — economy floor (3 000) |
| `tansy_13` | Tansy 1.3 | Compact | 2000s FF 1.3 supermini hatch — traffic / parked |
| `drayman_cargo` | Drayman Cargo | Truck | 2000s RWD 4.8 V8 full-size cargo van, body-on-frame — traffic / parked |
| `furrow_24` | Furrow 2.4 | Truck | 90s RWD 2.4 I4 compact pickup, body-on-frame — traffic / parked |
| `sentinel_courser` | Sentinel Courser | PoliceInterceptor | 2010s RWD 5.0 V8 highway-patrol pursuit coupe — fastest police unit on open road |
| `sentinel_shade` | Sentinel Shade | PoliceInterceptor | 2010s RWD 6.2 V8 unmarked full-size sedan, no push bar |
| `sentinel_marshal` | Sentinel Marshal | PoliceSuv | 2010s AWD 3.5 TT V6 unibody police utility, push bar |
| `sentinel_warden` | Sentinel Warden | PoliceSuv | 2010s 6.0 V8 extended full-size prisoner-transport van, body-on-frame |
| `sentinel_breacher` | Sentinel Breacher | PoliceSuv | 2010s armoured 4x4 tactical truck, 6.7 TD V8, 6.8 t — the battering ram |
| `gantry_box` | Gantry Box | Truck | 2010s medium-duty box truck, 6.7 TD I6, dual rears, 6.5 t — traffic |
| `longhaul_tractor` | Longhaul Tractor | Truck | 2010s long-nose sleeper semi tractor (bobtail), 12.8 TD I6, 8.8 t — traffic |
| `parcel_hi` | Parcel Hi | Truck | 2010s high-roof RWD delivery van, 2.1 TD I4 — traffic / parked |
| `quarry_tipper` | Quarry Tipper | Truck | 2010s tri-axle dump truck (unladen), 11 TD I6, 12 t — traffic |

The last three are civilian traffic cars (round 6b): cheap, slow, validated like the rest.
There is no `Van`/`Civilian` class yet, so the van and pickup use `Truck` (requested in
`docs/integration/cars.md`).

Round 9 added five police units and four heavy civilians. With no `Van`/`HeavyTruck` class
the heavy civilians are `Truck` and the van/armoured police units are `PoliceSuv` (contract
change request in `docs/integration/cars.md`). Heavy vehicles use the `commercial` tire
compound (`data/tires/commercial.json`: Fz0 20 kN, low load sensitivity), model dual rear
tires as one wide wheel per side, and put the rear axle of a tandem at the tandem centre (the
physics has four wheels; the renderer draws both axles). Truck top speeds are set by the rev
limiter in top gear, standing in for the fleet road-speed governor.

Naming check: `python3 tools/specsheet/nameplate_check.py` (exit 1 on any hit).
