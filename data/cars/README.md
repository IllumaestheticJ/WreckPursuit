# data/cars — vehicle definitions

One `VehicleDef` JSON per car: `data/cars/<id>.json` (`id` must equal the file stem). Schema:
`schemas/vehicle_def.schema.json`. Load + cross-reference check: `wp_contracts::load_car`.
Roster work is owned by workstream 09; the debug cars belong to the integration lead.

## Naming policy

**Real cars, real names** (owner decision 2026-10-29, car overhaul; replaces the old
analog-names-only rule and its `nameplate_check.py`). Cars model real-world makes, models and
trims as accurately as the data allows.

- `display_name` is the real name including the trim: `"Nissan Skyline GT-R (R34)"`.
- `id` is `<make>_<model>_<generation>_<trim>` in snake_case: `nissan_skyline_r34_gtr`.
- `analog_of` names the real car it models and the model year range.
- Spec sheets cite their sources (manufacturer data, period road tests).
- The pre-overhaul roster cars keep their invented names until each is replaced.

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
| `stoat_16v` | Stoat 16V | Compact | early-90s FF 2.0 16V NA hot hatch — torquey, lift-off happy |
| `bulwark_400` | Bulwark 400 | Muscle | early-70s 6.6 L HO V8 four-door sedan, clutch-LSD live axle, early radials |
| `arashi_t4` | Arashi T4 | Sport | mid-90s AWD 2.0 T rally-homologation saloon, 40/60 centre diff, rear LSD |
| `satchel_20` | Satchel 2.0 | Truck | 2000s FWD 2.0 I4 high-roof panel van, unibody — delivery traffic / parked |
| `fulmine_v10` | Fulmine V10 | Exotic | 2010s mid-engine 5.2 NA V10, permanent AWD (30 % front), dual-clutch — top of the market |
| `sentinel_warden` | Sentinel Warden | PoliceCruiser | late-80s RWD 5.7 V8 body-on-frame police sedan, 4-speed auto, push bar — low-heat old fleet |
| `sentinel_vigil` | Sentinel Vigil | PoliceInterceptor | 2010s RWD 5.0 V8 muscle-coupe pursuit car, push bar — high-heat (L4–L5) unit |
| `halberd_40` | Halberd 40 | Exotic | 2020s mid-engine 4.0 TT flat-plane V8, carbon tub, RWD, dual-clutch, active wing, factory track tyres |
| `ombra_v12` | Ombra V12 | Exotic | 2010s mid-engine 6.5 NA V12 flagship, AWD 25 % front, single-clutch automated manual |
| `ostwind_6t` | Ostwind 6T | Exotic | 2020s rear-engine 3.8 TT flat-six, AWD 30 % front, 8-speed dual-clutch — toughest supercar (dur. 4) |
| `kagero_hx` | Kagero HX | Exotic | 2010s mid-engine 3.5 TT V6 hybrid, AWD 35 % front (electric front axle), 9-speed dual-clutch |
| `morrigan_55` | Morrigan 5.5 | Exotic † | 2010s front-mid 5.5 NA V8 GT endurance racer, sequential transaxle, roll cage, big wing |
| `nyx_proto` | Nyx Proto | Exotic † | 2010s closed-cockpit sports prototype, 4.2 NA V8, < 1 t, highest downforce in the game |
| `merlo_20t` | Merlo 2.0T | Exotic † | 2010s FWD 2.0 T touring-car racer, sequential, plated front LSD, roll cage (dur. 4) |
| `mesquite_pr` | Mesquite PR | Truck | 2010s full-size 6.2 V8 pre-runner pickup, RWD, long-travel (0.33-0.35 m), 35-inch all-terrains |
| `erg_hamada` | Erg Hamada | Truck | 2020s rally-raid prototype, 5.0 NA V8, fixed 50/50 4WD, tube spaceframe, 37-inch all-terrains |

† Race cars are filed as `Exotic` until a `VehicleClass::Race` exists (requested in
`docs/integration/cars.md`, wave 3): the Sport purse could not price a slick-shod touring car
against the Exotic market. Race cars and the two off-roaders are **wave 3** (2026-10-23); they use
the build knobs `downforce`, lattice `cell_scale` (roll cage / composite tub) and `nose_scale`
(crash nose) described in `tools/specsheet/README.md`.

`tansy_13`, `drayman_cargo` and `furrow_24` are civilian traffic cars (round 6b); `satchel_20` joins
them as a delivery van: cheap, slow, validated like the rest.
There is no `Van`/`Civilian` class yet, so the vans and pickup use `Truck` (requested in
`docs/integration/cars.md`).
| `marten_20` | Marten 2.0 | Compact | early-90s FF 2.0 16V NA hot hatch — torquey alternative to the Vireo |
| `brigand_400` | Brigand 400 | Muscle | mid-70s 6.6 L V8 full-size four-door, live axle, LSD, period brakes |
| `talus_20t` | Talus 2.0T | Sport | late-90s AWD 2.0 T rally-homologation saloon, fixed 45/55 split, front + rear LSD |
| `tumbril_20` | Tumbril 2.0 | Truck | 2000s FWD 2.0 I4 high-roof panel van, unibody — delivery / traffic |

The last three are civilian traffic cars (round 6b): cheap, slow, validated like the rest.
There is no `Van`/`Civilian` class yet, so the vans and the pickup use `Truck` (requested in
`docs/integration/cars.md`). The last four were added in round 9.
