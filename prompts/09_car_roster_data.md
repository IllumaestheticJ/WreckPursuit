# PROMPT 09 — Car roster: 10 unlicensed analog cars with real-world-derived specs, tires, lattices (data only, `data/cars/`, `data/lattices/`)

Read `CLAUDE.md`, `docs/INTEGRATION.md`, `docs/CONTRACT_CHANGELOG.md`, `data/cars/README.md`. You own `data/cars/`, `data/lattices/` (car files only — not the generator), and `tools/specsheet/`. You may read `wp_contracts`, `wp_vehicle`, `wp_damage` (run `gen-lattice`). Branch `ws/cars`. No engine code changes.

## Goal
Ten driveable, physically plausible cars that **mimic real-world counterparts without any trademarked names, badges, model names, or recognisable liveries**, each with a full `VehicleDef`, tire assignment, a `LatticeDef` generated and then hand-adjusted, a one-paragraph design note, and a validated spec sheet. Plus three police units. Values must come from publicly available engineering/spec data for the analog class, with the derivation written down.

## Roster (analog → our name; adjust names freely, keep the archetype)
Racers:
1. Late-90s Japanese FF hot hatch, 1.6 L NA, ~1050 kg → **"Vireo Si"** (Compact, FWD) — the beater-tier hero.
2. Mid-2000s Japanese FF sedan, 2.0 L turbo, ~1400 kg → **"Kestrel Ralli"** (Sport, AWD variant available via mod)
3. Early-2000s Japanese FR coupe, 2.0 L turbo, ~1250 kg → **"Shiro S"** (Sport, RWD, LSD)
4. 90s Japanese FR GT, 3.0 L twin-turbo, ~1550 kg → **"Hakone GT"** (Sport, RWD)
5. 2010s American muscle, 5.0 L V8, ~1700 kg → **"Bronco-less" → "Marauder 5.0"** (Muscle, RWD)
6. 70s American muscle, big-block, ~1650 kg, live axle → **"Ironside 440"** (Muscle, RWD) — durable, heavy, scary brakes
7. 2000s German FR sedan, 3.2 L I6, ~1550 kg → **"Falke M"** (Sport, RWD)
8. 2010s German AWD hatch, 2.0 T, ~1500 kg → **"Lindwurm R"** (Sport, AWD clutch-pack on-demand)
9. 90s Italian mid-engine exotic analogue, 3.5 L, ~1300 kg → **"Strega 350"** (renamed away from a real nameplate) (Exotic, RWD, MR — needs `inertia`/CG done right)
10. Full-size pickup, 5.3 L V8, ~2400 kg → **"Hauler 1500"** (Truck, RWD/4WD) — the battering ram, durability over speed
Police:
11. Full-size RWD sedan cruiser (3.7 L V6, ~1900 kg) → **"Sentinel Cruiser"**
12. AWD performance interceptor (3.5 L TT V6, ~2000 kg) → **"Sentinel Interceptor"**
13. Full-size SUV (5.3 L V8, ~2600 kg) → **"Sentinel Tahoma SUV"**

Rename anything that comes out too close to a real nameplate. Run a final pass: grep all data and comments for a list of common real nameplates/brands and fail if any appear.

## For each car
- **Spec sheet** (`data/cars/<id>.spec.md`): curb mass, distribution, wheelbase, tracks, CG height (derive from class: ~0.45–0.5 m sedans, 0.4 m sports/exotics, 0.65–0.75 m trucks/SUVs), inertias (use standard approximations from mass + dims; show the formula), torque curve (10–15 points, shape from class/dyno norms, peak numbers class-typical), redline, gearing + final drive, diff types, tire sizes → radius/width and compound, spring rates/damping/ARB (class-typical; derive natural frequencies 1.1–1.6 Hz street, 1.8–2.2 sports), brake torques from rotor size & pad class, steering ratio & lock, CdA, lift. Cite the type of source (manufacturer spec class data, owner dyno averages, engineering tables) — not links to brand pages.
- **`VehicleDef` JSON** validating against `schemas/`.
- **Lattice**: run `gen-lattice` with the body profile and dims, then hand-adjust: engine/radiator positions for MR vs FR vs FF, fuel tank location, pillar materials (older cars: mild steel pillars, weaker cabin → higher Simulation lethality; modern: HSS/UHSS cabin cell, long crumple zones), truck ladder frame (separate `FrameLeft/Right` rails with high area), police push-bar as a reinforced front detachable region.
- **Durability class** 1–5 and a short "how this car breaks" note (e.g. Ironside: panels dent for days but the live axle tears off its mounts; Strega: everything behind the cabin is sacrificial).

## Validation
Use `apps/simcli` with prompt 02's metrics scenarios on each car (0–100, braking, skidpad, top speed) and prompt 03's barrier scenarios. Put results in `data/cars/VALIDATION.md` with pass/fail against class expectations (e.g. Vireo 0–100 ≈ 8.5–9.5 s; Marauder ≈ 4.5–5 s; Hauler ≈ 7.5 s; Strega skidpad ≥ 1.0 g). If the sim isn't merged yet on your branch, leave the table ready and run on the lead's integrated build later.

`tools/specsheet/`: a small script that renders a Markdown table of the whole roster from the JSON (mass, power, 0–100 est., class index estimate) for the design docs.

## Deliver
13 cars (JSON + spec.md + lattice), `VALIDATION.md`, roster table, `docs/integration/cars.md` (what the generator couldn't do by hand-edit and what `gen-lattice` should learn — pass to prompt 03's owner; any `VehicleDef` fields the data wanted but the schema lacks → contract requests).
