# Car roster integration notes

- **Workstream / prompt:** 09 — car roster data
- **Branch:** `ws/cars`
- **Crates / folders owned:** `data/cars/` (roster files), `data/lattices/` (roster car files, not the generator), `tools/specsheet/`
- **Contract version built against:** 0.1.0

## Response to lead feedback (run 2)

| Item | Done |
|---|---|
| 1 Tires | Rebuilt: 10 cars now on their intended compound (`sport` ×7, `police_pursuit` ×3). Hauler (`all_terrain`), Ironside (`vintage_bias`) and the two `street` cars stay on `street`. Sim columns of `VALIDATION.md` filled from `tools/specsheet/validation_results.json`, written by the new `collect_results.py` (runs `wp_vehicle`'s metrics example + `damage-barrier --sweep`). |
| 2 Rampart / Wolfhound / Ironside | **Wolfhound 100-0: 61.7 → 39.9 m** (in band), Rampart 55.6 → 39.0 m, Cruiser 57.0 → 40.1 m — two causes, below. **Rampart 0-100: 6.3 → 5.9 s**, still over 4.5-5.0: it is not only traction (semi-slicks only reach 5.5 s; final drive, CG, LSD each ≤ 0.03 s) — `wp_vehicle`'s launch is conservative for powerful RWD (its debug coupe at 200 kW/t runs 5.2 s). Not fixed by inflating power; flagged to 02. Ironside 52.5 m: intended "scary brakes", in its band. |
| 3 Brake semantics | Re-derived for 02's model ("favoured axle full `max_torque_*`, other × (1−b)/b"). `max_torque_*` = caliper capacity per wheel; `bias_front` is now the proportioning valve: 0.5 when the rear caliper is the limit (both axles full), above 0.5 only to trim an oversized rear to the front's lock margin. Old data (b ≈ 0.7) starved the rear to ~40 %. Plus an **ABS ceiling**: delivered front ≤ 1.5 × front lock torque on the car's own tire (rear scaled with it) because 02's ABS releases to 45 % of the *commanded* torque — an oversized caliper keeps the wheel locked (measured on the Wolfhound: 5770 N·m → 47.3 m, 4500 N·m → 39.9 m). Spec sheets show raw capacity, the cap and delivered torques. |
| 4 `junkyard_beater` | Added: Beater, base price 3 000, late-80s FF 1.6 SOHC sedan, worn (tired engine, 0.18 damping ratio, slow synchro). Sim: 14.0 s 0-100, 0.83 g, 39.1 m, 163 km/h. |
| 5 / run 2 gen-lattice | All 14 lattices regenerated with `gen-lattice` (profile, real car mass, `--strength` from durability class 1-5 → 0.80/0.90/1.00/1.15/1.30, front overhang per car). The box-grid adjustments it covers are dropped (shaping, crumple rows, pillars, radiator/engine separation, doors, bed, node mass). What is still applied on top is listed below and in each spec sheet. Barrier sweep run on all 14 (VALIDATION.md): 109/112 checks pass; the 3 misses are one damage-model finding for 03 (two-box rear). |

## Exposes

- 14 cars, each `data/cars/<id>.json` (VehicleDef) + `data/cars/<id>.spec.md` + `data/lattices/<id>.json`:
  racers `vireo_xs`, `kestrel_ts`, `shiro_turbo`, `hakone_gt`, `rampart_50`, `ironside_440`,
  `sperber_32`, `lindwurm_4t`, `lucciola_350`, `hauler_1500`; police `sentinel_cruiser`
  (PoliceCruiser), `sentinel_wolfhound` (PoliceInterceptor), `sentinel_bastion` (PoliceSuv);
  economy floor `junkyard_beater` (Beater, 3 000). All load via `wp_contracts::load_car`.
- `data/cars/ROSTER.md` — roster table; `data/cars/VALIDATION.md` — sim + estimate vs class
  targets and the barrier matrix, with every miss explained.
- `tools/specsheet/` — `build_roster.py`, `collect_results.py` (sim metrics + barriers →
  `validation_results.json`), `specsheet.py`, `estimate.py`, `nameplate_check.py`,
  `smoke_sim.py` (simcli format v2). See `tools/specsheet/README.md`.
- Police push bar: 4 extra `FrontBumper` nodes ahead of the bumper, detachable **group 8**
  (moved from 3: gen-lattice uses 0-7 for bumpers, hood, trunk and doors), 15 000 N·s, 25 kg.
  Cruiser and Wolfhound also get gen-lattice's `cruiser` profile (reinforced rails, ×2 bumper
  beam); the Bastion (SUV) gets the ×2 bumper beam by hand.

## Needs

| From | What | Workaround now |
|---|---|---|
| 02 vehicle | **ABS should modulate relative to the wheel's lock torque** (or reduce pressure until slip recovers), not release to a fixed 45 % of commanded torque. Any car whose caliper exceeds ~1.6× its lock torque stops 7-15 m long. | ABS ceiling in the car data (`ABS_CEILING = 1.5` in `build_roster.py`); remove when fixed. |
| 02 vehicle | Launch/TCS for powerful RWD cars is conservative (Rampart 5.9 s; debug coupe 5.2 s at 200 kW/t vs ≈ 4.6 s class). Sim also stops ≈ 1-2 m longer than road tests. | Explained in VALIDATION.md (cause "A"); car data not inflated. |
| 02 vehicle | `all_terrain` (Hauler) and `vintage_bias` (Ironside) compounds. | `street`. |
| 02 / lead | Lucciola skidpad 0.95 g on `sport` vs prompt-09 target ≥ 1.0 g (1.09 g on `semi_slick`, measured). Decide: stickier `sport`, semi-slicks on the Lucciola, or accept 0.95 (realistic for the archetype). | Left on `sport`. |
| 03 damage | **Two-box rear structure**: on the hatch and SUV profiles the passenger cell runs to the tailgate, so a 40 km/h rear barrier gives 0.22-0.33 m cabin intrusion and a total loss (Vireo, Lindwurm, Bastion). Sedans/coupes are fine. | Reported only; not patched in car data. |
| 03 damage | `gen-lattice` options for what is still hand-applied (list below), `uhss` material. | `adjust_lattice` in `build_roster.py`. |
| 03 damage | `SEC` in `build_roster.py` mirrors `lattice_gen::DEFAULT_SECTIONS` to recognise beam roles; if those sections change, the era adjustment silently stops matching. A role tag per beam (or exporting the sections) would remove the coupling. | Keep in sync by hand. |
| 08 economy | Mods: Kestrel AWD conversion, Hauler 4WD (should land its 0-100 in band), Ironside LSD + disc-brake conversion, tire upgrades. | Not created (data/mods is 08's). |
| 10 harness / lead | Run `nameplate_check.py` in CI. | Run by hand before each push. |

## Contract change requests

Decisions are in "Lead feedback" below; status updated here.

| # | Change | Status |
|---|---|---|
| 1 | `DrivetrainDef::torque_converter` | accepted (v0.2.0) — Hauler/Cruiser 0-100 misses wait on it |
| 2 | On-demand AWD | accepted (merged with 02 #5) |
| 3 | Rear drum + fade | deferred |
| 4 | `BrakeDef` doc | accepted (doc only); roster re-derived for 02's semantics (above) |
| 5 | Solid axle | deferred |
| 6 | Roll centre height | deferred |
| 7 | `BodyRegion::Accessory` (push bar) | accepted in principle; push bar now uses detach group 8 until then |
| 8 | `fuel_capacity_l` | accepted; values ready to add once the field exists (tank sizes are class-typical: 45-70 L cars, 90-100 L truck/SUV) |

Non-contract data request: `data/materials/uhss.json` (03) — 2010s cabins still use gen-lattice's HSS.

## Hand adjustments still applied on top of gen-lattice (for 03)

Each spec sheet ends with the exact list for that car. In order of how much a generator option
would help:

1. **Era of the passenger cell** — cabin members, sills, pillars, roof rails and cabin
   cross-members: 70s → mild steel × 1.0 section (+ door beams × 0.5), 80s HSS × 0.65,
   90s × 0.75, 2000s × 0.90, 2010s unchanged. This is what makes Simulation lethality differ by era.
2. **Mid-engine** (no gen-lattice profile): coupe body, engine block moved behind the cabin
   bulkhead and remounted to the rear rails + bulkhead, rear-of-bulkhead nodes relabelled
   `FrameRear`, aluminium rear subframe (sacrificial), gearbox/half-shafts at the rear axle,
   side fuel tanks at the sills.
3. **Ladder frame** for truck/SUV: cab-floor rail nodes relabelled `FrameLeft/Right` and the
   under-cab rail beams given rail sections so the frame runs bumper to bumper.
4. **Push bar** as a detachable accessory (group 8); ×2 bumper beam for police bodies that are
   not the `cruiser` profile (SUV).
5. **Material options**: steel hood (gen-lattice always uses aluminium), chrome steel bumpers
   (pp → mild steel, detach impulse ×2.3), all-aluminium panels (MR).
6. **Component placement**: transverse FF gearbox on the engine's left end (gen-lattice puts
   it in the FR tunnel), 70s fuel tank behind the rear axle, truck side-saddle tank, Ironside
   live-axle mounts (rear suspension fail 0.04 m, wheel 0.07 m).
7. Crew cab: the Hauler is a crew-cab pickup but the truck profile is two-door (not adjusted).

## Changes outside my crates

- None. `tools/specsheet/smoke_sim.py` (mine) has the lead's format-v2 update.

## Perf numbers

| Measurement | Machine | Native | Browser | Budget |
|---|---|---|---|---|
| Lattice size per roster car | — | 244-304 nodes, 1539-1988 beams | same | prompt 03: 0.25 ms/car lattice step (03's `damage-bench` covers it) |
| `build_roster.py` (14 cars incl. gen-lattice runs) | cloud container | 0.8 s | — | — |
| `collect_results.py` (metrics + 14 barrier sweeps) | cloud container, release | ≈ 15 s | — | — |

## Known gaps

- Sim misses (VALIDATION.md, all explained): small "sim conservative" overruns (Kestrel,
  Hakone, Sperber 0-100 by 0.02-0.3 s; Hakone/Sperber/Lindwurm/Rampart 100-0 by 0.5-1 m),
  Rampart 0-100 and skidpad, Lucciola skidpad (decision above), automatics' 0-100 (torque
  converter pending), three tire-limited skidpads 0.01-0.02 g under.
- The ABS ceiling and the `SEC` mirror are workarounds (see Needs).
- AWD on-demand, automatics, live axles, drum fade are approximated (contract requests 1-3, 5).
- Prices and durability classes are first-pass; 08 should tune prices against the economy.

## Lead feedback (2026-10-19)

**Merged** (`7dbf8f3`). `nameplate_check.py`: 54 files, 0 problems. All 13 cars load, validate and run
deterministically in both modes on the **real** `wp_vehicle` model (`smoke_sim.py` green).

Lead change in your area: `tools/specsheet/smoke_sim.py` now writes simcli scenario **format v2**
and calls `simcli run ... --no-write` (the harness changed the format and CLI). Pull it.

First integrated numbers (`cargo run --release -p wp_vehicle --example metrics`, cloud container;
most cars are still on `street` because they fell back to it):

| car | 0-100 s | 100-0 m | skidpad g | Vmax km/h |
|---|---|---|---|---|
| vireo_xs | 9.87 | 45.7 | 0.89 | 202 |
| kestrel_ts | 7.41 | 46.3 | 0.86 | 247 |
| shiro_turbo | 6.68 | 42.4 | 0.88 | 258 |
| hakone_gt | 6.16 | 40.5 | 0.83 | 283 |
| rampart_50 | 6.29 | 55.6 | 0.81 | 270 |
| ironside_440 | 7.18 | 61.4 | 0.74 | 212 |
| sperber_32 | 6.34 | 41.9 | 0.84 | 274 |
| lindwurm_4t | 5.53 | 43.9 | 0.86 | 260 |
| lucciola_350 | 4.94 | 42.8 | 0.87 | 290 |
| hauler_1500 | 8.95 | 49.8 | 0.67 | 204 |
| sentinel_cruiser | 8.74 | 57.0 | 0.77 | 236 |
| sentinel_wolfhound | 6.36 | 61.7 | 0.80 | 243 |
| sentinel_bastion | 8.47 | 51.3 | 0.66 | 213 |

Next for you:
1. `data/tires/` now has `sport`, `semi_slick`, `police_pursuit`, `wet_biased` → re-run
   `build_roster.py` so cars switch to their intended compounds, then fill the "sim" columns of
   `VALIDATION.md` and write `tools/specsheet/validation_results.json` from the metrics example.
2. Look at: Rampart 0-100 6.3 s vs 4.5-5.0 band (traction, as you predicted); Wolfhound 100-0
   61.7 m is long for an interceptor; Ironside 61.4 m is the intended "scary brakes".
3. ⚠ Brake semantics: 02 implemented "biased axle gets full `max_torque_*`, the other × (1−bias)/bias".
   Your data assumed capacity per wheel with bias = pedal split. 02 will add a worked example to its
   README; re-check your brake numbers against it.
4. Economy (08) asks for a floor car `junkyard_beater` (Beater class, base price 3 000).
5. barrier/crash columns wait on 03 (damage) — the sim now delivers contacts to damage.

Contract requests (decisions; implemented together in v0.2.0 unless noted):
| # | Decision |
|---|---|
| 1 torque converter | Accepted (`DrivetrainDef::torque_converter: Option<...>`). |
| 2 AWD on-demand | Accepted, merged with 02 #5. |
| 3 rear drum + fade | Deferred (post Checkpoint 1; brake-fade model is new vehicle work). |
| 4 brake doc | Accepted, doc only — see item 3 above. |
| 5 solid axle | Deferred (post Checkpoint 2). |
| 6 roll centre | Deferred (low). |
| 7 `BodyRegion::PushBar`/`Accessory` | Accepted in principle as a generic `Accessory` region; final shape waits on 03's region list. |
| 8 `fuel_capacity_l` | Accepted, `Option<f32>`, fuel mass included in `mass_kg` at 50 % fill. |

### Run 2 (2026-10-19)
- Machine B's `gen-lattice` is merged (profiles, eras, crumple rows, pillars). B asks you to regenerate
  every roster lattice with it (profile, real mass, `--strength` for durability class) and drop the
  hand adjustments it now covers — see `crates/wp_damage/README.md`.
