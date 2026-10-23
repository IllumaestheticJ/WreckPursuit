# Car roster integration notes

- **Workstream / prompt:** 09 — car roster data
- **Branch:** `ws/cars`
- **Crates / folders owned:** `data/cars/` (roster files); vehicle-models wave 1: multi-model hooks in `wp_sim::{police_link,traffic_link}`, `wp_world::{lib,traffic}`, `wp_wasm`, `apps/simcli/src/run.rs`, `data/lattices/` (roster car files, not the generator), `tools/specsheet/`; round 8: `data/tires/` edits the lead assigned to H (`PaintedLine`, `treaded`)
- **Contract version built against:** 0.4.0 (wave 3)

## Wave 3 (2026-10-23, owner request) — 4 supercars, 3 race cars, 2 rally-raid / pre-runner

Owner request: "add 4 more highly detailed supercars, 3 dedicated race cars, and 2 rally raid /
pre-runner style vehicles". Full pipeline per car: `roster_inputs.py` entry → VehicleDef, spec
sheet (every derivation), lattice; sim validation (metrics + 8-barrier sweep, both modes);
smoke-sim determinism (both modes, identical hashes); audio def; panel spec; procedural mesh with
a per-car body kit (`tools/assets/src/styles/performance/{supercar,race,offroad}.rs`) and a
mechanical-spec row; menu stats (`data/stats/cars.json`); AI capability row
(`wp_opponent::capability_table`). Contract **0.4.0**, no contract change.

| Id | Class | Mass (sim) | Power | 0-100 sim | Vmax sim | 100-0 | Skidpad | Barriers | Dur. | Price |
|---|---|---|---|---|---|---|---|---|---|---|
| `halberd_40` | Exotic | 1488 kg | 513 kW (4.0 TT flat-plane V8) | 4.16 s | 347 km/h | 33.3 m | 1.16 g | 8/8 | 3 | 125 000 |
| `ombra_v12` | Exotic | 1692 kg | 517 kW (6.5 NA V12) | 3.46 s | 348 km/h | 35.6 m | 0.99 g | 8/8 | 3 | 124 000 |
| `ostwind_6t` | Exotic | 1690 kg | 470 kW (3.8 TT flat-six, rear-engine) | 3.76 s | 340 km/h | 36.1 m | 0.96 g | 8/8 | 4 | 110 000 |
| `kagero_hx` | Exotic | 1833 kg | 418 kW (3.5 TT V6 + electric, combined) | 3.93 s | 326 km/h | 36.2 m | 0.96 g | 8/8 | 3 | 100 000 |
| `morrigan_55` | Exotic (race) | 1288 kg | 425 kW (5.5 NA V8, restricted) | 4.28 s | 291 km/h | 32.1 m | 1.21 g | 8/8 | 3 | 120 000 |
| `nyx_proto` | Exotic (race) | 977 kg | 419 kW (4.2 NA V8, restricted) | 4.00 s | 311 km/h | 31.9 m | 1.37 g | 7/8 | 3 | 125 000 |
| `merlo_20t` | Exotic (race) | 1238 kg | 252 kW (2.0 T I4, FWD) | 5.17 s | 241 km/h | 34.4 m | 1.10 g | 8/8 | 4 | 100 000 |
| `mesquite_pr` | Truck | 2759 kg | 317 kW (6.2 NA V8) | 8.14 s | 197 km/h | 46.9 m | 0.66 g | 8/8 | 5 | 32 000 |
| `erg_hamada` | Truck | 2113 kg | 308 kW (5.0 NA V8, restricted) | 6.21 s | 201 km/h | 44.1 m | 0.66 g | 8/8 | 5 | 32 000 |

- **Supercars** — each with a different drive/gearbox so they play differently: Halberd = RWD
  on factory track tyres (`semi_slick`), dual-clutch, active wing (`downforce` 0.10 / 0.25);
  Ombra = AWD 25 % front, single-clutch automated manual (0.22 s shifts); Ostwind = rear engine
  (61 % rear weight, mid-engine lattice + `nose_scale` 1.8 for its front luggage-bay crush
  zone), AWD 30 % front; Kagero = hybrid, electric torque folded into a flat crank-equivalent
  curve, AWD 35 % front.
- **Race cars** — sequential boxes (0.15 s), plated LSDs, `semi_slick` as the stand-in for
  slicks, wings (`downforce` 0.40 / 0.85 GT, 0.60 / 0.80 prototype + −0.9 / −1.1 body lift,
  0.05 / 0.25 touring), roll cage / carbon cell (`cell_scale` 1.6 / 1.8 / 1.6). Filed as
  **Exotic**: in Sport, `wp_economy`'s purse caps the touring car near 54 000, where its
  slick-tyre class index (682) beats the 120 000 Lucciola (619) and fails
  `pricier_cars_are_not_slower`. The class index saturates at 999 for Halberd / Morrigan / Nyx.
- **Off-road** — Mesquite: ladder frame, steel tube bumpers, 0.33 / 0.35 m travel, 1.15 / 1.35 Hz,
  RWD + locker (stiff clutch LSD), converter automatic. Erg: tube spaceframe (`ladder_frame` +
  `cell_scale` 1.8 on the `suv` lattice), fixed 50/50 4WD, plated diffs both ends, 0.35 m travel,
  300 L fuel cell (112 kg at half). Both priced at the Truck-band ceiling the front-hit/purse
  test allows (32 000, same as the Hauler).
- **Sim misses** (explained in `VALIDATION.md`): 0-100 for Halberd, Ostwind, Kagero, Morrigan,
  Nyx and 100-0 for Ombra, Ostwind, Kagero — shared cause A (TCS-on launch, ABS cycle), plus
  race gearing for the race cars. **Barrier miss:** Nyx frontal 56 km/h crush ≈ 1.3-1.4 m (band
  0.35-1.3; total loss, cabin intact) — see the request below.
- **Ostwind drivetrain:** `AwdOnDemand { base_front_split: 0.10, max_rear_split: 0.50 }` never
  closed at launch (4.85 s); the type is built for front-biased cars. Fixed `Awd 0.30` instead.
- **Fixed in passing:** `collect_results.py` still checked barriers against the `sim` rows after
  contract v0.4.0 renamed the old Simulation to `extreme`, so *every* car showed 5-6 barrier
  FAILs; it now reads `extreme` (and compares "Arcade milder" against `sim`). The roster is
  back to its pre-v0.4.0 matrix (only the known Wolfhound/Harrier offset near-misses).
- **Free-roam haunts not changed:** adding the new cars to `wp_opponent::roam::haunt_cars` would
  reshuffle the stock roaming roster and move race goldens, so it is left to the opponent
  owner. Suggested: Highway + `halberd_40`, `ombra_v12`; Touge + `ostwind_6t`, `kagero_hx`;
  Dirt + `erg_hamada`, `mesquite_pr`; Downtown + `merlo_20t`. The race cars want a track/event
  haunt rather than street haunts.
- **Menu stat bars** saturate (1.0) for the race cars' cornering and the supercars' top speed
  (`data/stats/cars.json` scales 0.55-1.05 g, 140-310 km/h); UI owner may want wider scales.

### Changes outside my folders (wave 3)

- `crates/wp_wasm/src/race_spots_tests.rs` (testbed): `city_chase_in` → `night_run_in`. Main
  did not compile its tests after merging #50 (Ashpeak) and #51 (City chase → Night run rename).
- `crates/wp_vehicle/tests/gearbox.rs` (02): the fast-shift whitelist gains the seven new
  dual-clutch / automated-manual / sequential ids (0.15-0.22 s, inside its 0.12-0.25 band).
- `crates/wp_opponent/src/capability_table.rs` (generated, `WP_WRITE_CAPS=1`), `data/stats/cars.json`
  (generated, `simcli car-stats`), `data/audio/cars/*` (one def per new car; V12 uses the V8 pack,
  flat-six the I6 pack — no V12 / flat pack yet).
- `tools/assets` (R): `mechanical/spec.rs` rows, `styles/performance/{supercar,race,offroad}.rs`,
  dispatch in `styles/performance/mod.rs`, panels + meshes for the new ids only, and a new
  `tools/assets/mesh_shots.mjs` (rest-pose PNGs of a mesh, no testbed / wasm build needed).

### Models (wave 3)

Per-car body kits in `tools/assets/src/styles/performance/{supercar,race,offroad}.rs` (lamps
with a distinct tail pattern per car, intakes, vents, louvres, splitters, wings, diffusers,
number panels / stripes as plain colour patches, tube bumpers, light bars, chase rack, spare
wheels, sand ladders, window nets, …). Hero tier, all under the 20k budget: halberd_40 18,586, ombra_v12 18,670, ostwind_6t 17,148, kagero_hx 17,626, morrigan_55 19,136, nyx_proto 18,388, merlo_20t 18,156, mesquite_pr 19,527, erg_hamada 18,536 tris.
Kit-helper gaps found (worked around locally): no dome/blister or tube helper, `louvres` /
`wing` / `diffuser` / `splitter` use fixed part names (once per car), `top_stripe` density is
not settable, `mud_flaps` is all-four only. `Tips::HighCentre` draws four touching tips in a row
(also on `fulmine_v10`); `nyx_proto` uses `Centre`, `halberd_40` `QuadHigh`. The pre-runner's
bumper light bar and exo-bumper pods are detail parts in lamp materials (the four-lens test
counts light parts per end), so they may not switch on with the head/tail lamps.

### New requests (wave 3)

| # | To | Request |
|---|---|---|
| 10 | lead (contract) | `VehicleClass::Race` (race-built cars: own purse, market band, no traffic use) and `VehicleClass::Offroad` (or reuse a `Van`/`Civilian` split, request 9) so the rally-raid car and pre-runner are not priced and bracketed like the Hauler. |
| 11 | 02 (tires) | A `race_slick` compound (lat D ≈ 1.45-1.55, wet ≈ 0.35, no tread): the three race cars run `semi_slick` as a stand-in. A sand/dune-biased `desert_at` would suit the two off-roaders. |
| 12 | 03 (damage) | A flat sports-prototype profile in `lattice_gen` (cab-forward tub, low nose cone): `nyx_proto` uses the mid-engine coupe lattice and its 1 m nose folds past the 56 km/h crush band. |
| 13 | 08 (economy) | An Exotic purse tier for class-index ≥ 800 (repeat of the Fulmine request) — the wave-3 supercars are compressed to 100 000-125 000 for the same front-hit/purse reason. |
| 14 | 13 (audio) | V12 and flat-six engine packs (Ombra uses `engine_rec_v8`, Ostwind `engine_rec_i6`); an electric whine layer for the hybrid Kagero. |

## Lead helper note (`lead/handling-balance`, playtest row 28) — no car data changed

"All cars understeer too much" was the keyboard/gamepad steering mapping and ESC, not car
data (fixed in `wp_vehicle`, see `docs/integration/vehicle.md` → "Handling balance"). For the
record, so H can judge setup changes against measurements (`cargo run --release -p wp_vehicle
--example handling`, wheel preset = chassis, with A's differentials):

- Chassis understeer gradient at 80 km/h: RWD 0.07–0.35 deg/g, AWD 0.32–0.33, FWD 0.35–0.63
  (junkyard_beater 1.42), heavy 0.49–1.77. Regression ranges per class are in
  `crates/wp_vehicle/tests/handling_balance.rs` (a setup change that leaves them fails there).
- Anti-roll bars are a weak lever here (measured on main before the diffs): front −40 % / rear
  +40 % on rampart_50, shiro_turbo, hakone_gt (−20/+20 % on kestrel_ts) moved the gradient
  ≤ 0.01 deg/g and the limit balance (front slip/peak ÷ rear) from 2.09–2.35 to 1.85–2.30. The
  diff model did more (RWD gradient 0.45–0.61 → 0.07–0.35), so `differentials.toml` is the
  knob for "RWD rotates at the limit", not bars.
- VALIDATION skidpad / 0-100 / 100-0 are not changed by this branch (metric runs are
  bit-identical), so not re-collected here.
## Lead request (2026-10-20) — stock shift times + shift mods (owner playtest row 97)

Merged `origin/lead/round12` at `517621f`. "The shifting is a little too fast for unmodified cars —
modifications should make shift speeds faster."

**1. Stock shift times, clutch gearboxes** (`roster_inputs.py` → car JSON; guide in
`tools/specsheet/README.md`): manual sports 0.42-0.45 s, economy / hot hatch 0.45-0.50, muscle
0.50, light-truck manual 0.65, beater 0.70, dual-clutch 0.18. Sports manuals sit at the top of
the lead's 0.35-0.45 guide so that fully modded (×0.28) lands at ≈ 0.12 s. The lead's
`debug_coupe` / `debug_sedan` are unchanged (lead-owned, pinned by base vehicle scenarios).

**Torque-converter automatics are deliberately NOT re-based yet** (ironside_440, hauler_1500,
drayman_cargo and the three police units keep 0.30-0.40 s; my proposed values are in the table).
Reason: a vehicle-model bug, below. With today's converter shift, a longer `shift_time_s` makes an
automatic *faster* and its shift *inaudible* — the opposite of row 97 on both counts. I measured
it, tried the proposed values (police 0.40-0.50 s: the cruiser's WOT shift dip falls to 0.8-1.0
dB, below J's 1.2 dB "audible" floor in `engine_shift.rs`), and reverted those six cars to their
exact previous data. They get the proposed values as soon as A's fix lands (one line each).

| car | box | stock s | fully modded s | sim 0-100 s before → now | ¼ mile s @ km/h |
|---|---|---|---|---|---|
| `vireo_xs` | manual | 0.25 → **0.45** | 0.13 | 9.47 → 9.78 | 17.1 @ 139 |
| `kestrel_ts` | manual | 0.25 → **0.42** | 0.12 | 7.13 → 7.37 | 15.4 @ 156 |
| `shiro_turbo` | manual | 0.22 → **0.42** | 0.12 | 6.45 → 6.56 | 14.8 @ 160 |
| `hakone_gt` | manual | 0.25 → **0.45** | 0.13 | 5.88 → 6.00 | 14.3 @ 169 |
| `rampart_50` | manual | 0.25 → **0.50** | 0.14 | 5.83 → 6.18 | 14.0 @ 176 |
| `ironside_440` | auto (converter) | 0.40 (kept; proposed **0.65**) | 0.11 | 8.18 → 8.18 | 16.2 @ 155 |
| `sperber_32` | manual | 0.22 → **0.42** | 0.12 | 6.05 → 6.33 | 14.3 @ 166 |
| `lindwurm_4t` | dual-clutch | 0.08 → **0.18** | 0.05 | 5.44 → 5.58 | 13.8 @ 166 |
| `lucciola_350` | manual | 0.22 → **0.45** | 0.13 | 4.53 → 4.69 | 12.9 @ 182 |
| `hauler_1500` | auto (converter) | 0.30 (kept; proposed **0.60**) | 0.08 | 8.10 → 8.10 | 16.2 @ 146 |
| `sentinel_cruiser` | auto (converter) | 0.30 (kept; proposed **0.50**) | 0.08 | 6.40 → 6.40 | 14.7 @ 164 |
| `sentinel_wolfhound` | auto (converter) | 0.30 (kept; proposed **0.45**) | 0.08 | 5.92 → 5.92 | 14.2 @ 164 |
| `sentinel_bastion` | auto (converter) | 0.30 (kept; proposed **0.55**) | 0.08 | 7.97 → 7.97 | 16.0 @ 144 |
| `junkyard_beater` | manual | 0.55 → **0.70** | 0.20 | 15.46 → 15.70 | 20.3 @ 112 |
| `tansy_13` | manual | 0.25 → **0.50** | 0.14 | 13.40 → 13.79 | 19.3 @ 120 |
| `drayman_cargo` | auto (converter) | 0.35 (kept; proposed **0.70**) | 0.10 | 10.86 → 10.86 | 17.7 @ 133 |
| `furrow_24` | manual | 0.35 → **0.65** | 0.18 | 10.82 → 11.26 | 18.1 @ 124 |

**Bug (→ A, 02 vehicle): on torque-converter automatics a longer shift is *faster*.**
`sentinel_cruiser` 0-100 at shift_time_s 0.1 / 0.3 / 0.5 / 0.8 / 1.2 s = 6.54 / 6.40 / 6.33 /
6.27 / 6.21 s (manual `vireo_xs`: 9.26 / 9.55 / 9.85 / 10.33 / 10.95 s, correct). Cause: during a
converter shift there is no torque cut, the lock-up clutch opens and `Gearbox::total_ratio`
blends the old and new ratios over the whole shift, so the box acts like a CVT holding the
engine near peak power for longer, and the rpm falls so gently that `wp_audio`'s rate-based
shift dip barely registers. A real automatic changes ratio in a short inertia phase (≈ 0.15-0.25
s) with a torque dip. Suggested fix (A's call): blend the ratio only over the last ≈ 30-40 % of
the shift and reduce engine torque during it, so shift time costs acceleration as it does for
clutch boxes. Until then the shift mods also make automatics slightly *slower*; G may want shift
mods to require a clutch gearbox (no such condition in the mod schema yet; `wp_economy` only).

**Where `shift_time_s` applies:** every shift path in `wp_vehicle` — manual paddles, the
`auto_gears` assist (player automatic and every AI driver: opponents, police, traffic) and the
converter boxes (`Gearbox::shift(target, def.drivetrain.shift_time_s)` in `vehicle.rs` /
`drivetrain.rs`); `wp_economy::estimate` uses it; mods scale it (`mods.rs`). **Audio:** J's
upshift dip and rev-match blip come from the engine's rpm rate and load each frame, and the gear
clunk fires when the gear actually changes, so they follow the shift time. One exception, fixed
(J, see below): the turbo **blow-off** stopped firing on WOT upshifts with the slower shifts.

**2. Mods** (`data/mods/drivetrain.json`, G's folder; data-only, the schema already supports
it). New line `shifter`: tier 1 **Short-shift Kit** (×0.85, 400) and tier 3 **Sequential
Shifter** (×0.5, 9 500, requires `drivetrain_gears_race`; "expensive and harsh"). Existing
×0.9 / ×0.8 (clutch line) and ×0.85 / ×0.7 (gearset line) unchanged. Lines multiply, tiers within
a line replace each other: ×0.85 (kit alone) … ×0.28 (stage-2 clutch + dog-box + sequential); a
0.42-0.45 s sports manual ends at 0.12-0.13 s, the 0.18 s dual-clutch at 0.05 s. `VALIDATION.md`
lists stock → fully modded per car (`specsheet.py` reads the factor from the mod file).

**3. Re-measured.** `collect_results.py` now also records the **quarter mile** (402.336 m,
elapsed time + trap speed, full sim via `simcli`, auto gears, TCS on;
`pursuit_probe.quarter_mile`), shown in `VALIDATION.md`. Clutch-gearbox cars lost 0.1-0.45 s to
100 km/h; two moved out of their bands — `vireo_xs` 9.78 s (8.5-9.5) and `tansy_13` 13.79 s
(11.5-13.5). Bands not widened and cars not tuned back (the owner asked for slower stock
shifts; the miss notes give before / after; the bands are road tests with real shifts, so the
remainder is A's conservative TCS launch). Barriers unchanged. Pursuit: the cruiser still
reaches the day cap before every cheap and traffic car (beater 5.0 s, Tansy 5.3 s, Furrow 5.6 s,
Vireo 6.1 s, Drayman 6.3 s at the cap).

**Tests and fixes outside my folders** (intent of each test unchanged):
- `wp_vehicle/tests/gearbox.rs` (A): `street_cars_shift_crisply` →
  `street_cars_shift_at_believable_stock_speeds` (clutch boxes 0.35-0.55 s, dual-clutch
  0.12-0.25 s, debug cars exempt; was "≤ 0.25 s", which row 97 overturns);
  `kickdown_on_full_throttle` waits shift time + 0.15 s (was a fixed 0.4 s);
  `shifts_are_rev_matched_and_the_torque_gap_is_short` allows 40 % of the shift + 0.04 s
  re-engagement (measured 0.208 s at 0.45 s; was 45 % flat).
- `wp_vehicle/tests/differentials.rs` (A): `clutch_lsd_adds_controllable_oversteer_on_corner_exit`
  pins `shift_time_s` 0.22 for its cars — an upshift inside the 2 s exit masked the diff
  (Lucciola +1.7° vs the +2° threshold; passes at 0.22 and 0.30).
- `wp_audio/src/engine.rs` (J): the blow-off detector's lift window 0.15 → **0.35 s**
  (`BLOWOFF_LIFT_WINDOW_S`). It fires when the throttle falls from above 0.5 to below 0.2 under
  boost within the window; a clutch shift ramps the throttle out over 40 % of the shift (≈ 0.13 s
  to below 0.2 at the Shiro's 0.42 s), longer than the old 75 ms, so the Shiro blew off on 0 of 4
  WOT upshifts. Now on every one; a slow driver lift (> 0.35 s) still does not blow off.
- `wp_audio/tests/engine_shift.rs` (J): `every_wot_upshift_dips_and_re_rises` skips the re-rise
  check when the scripted drive lifts the pedal inside its window (the Rampart's 5→6 now ends
  0.25 s before the script's fixed lift at 31.05 s; the audio is back at full level on
  re-engagement).
- No pinned hash changed. Goldens: standalone debug `simcli goldens`, old vs new data — only
  `cars_beater_accel` and `cars_beater_top_speed` newly fail; re-blessed (`[bless]`). The base's 49
  golden failures are unchanged with identical range-expectation failures.

**Open (→ L / lead): `wp_race race_picker::hollins_hill_sprint` now fails** ("only 2 of 4
finished": two AI racers captured at gate 2). Over `WP_RACE_SEED` 1-5: 11 of 20 racers finish
with the new shift times vs 16 of 20 before, mostly `Dnf(FinishWindow)` — the field spreads out
more (each car loses a different amount to its slower shifts) and stragglers miss the window
after the winner. Not tuned away in car data; the finish window or `min_finish` 3 is L's call.

### Changes outside my folders (row 97)
- `data/mods/drivetrain.json` (G): two new mods in a new `shifter` line (data only).
- `crates/wp_vehicle/tests/gearbox.rs`, `crates/wp_vehicle/tests/differentials.rs` (A): test
  adjustments above.
- `crates/wp_audio/src/engine.rs`, `crates/wp_audio/tests/engine_shift.rs` (J): blow-off lift
  window constant and the pedal-held guard above.

## Lead follow-up 2 (2026-10-20) — beater after A's ride + countersteer work

Merged `origin/lead/round12` at `7a2f19b`. Lead: `front_drive_cars_understeer_mildly` failed
(beater 1.602 °/g) on the combined tree. Band unchanged.

**What the merge did to the beater:** A's ride commit edited all 19 car JSONs directly from
an older checkout. The beater's suspension block came in with its *pre-retune* springs and travel
(20 900 / 15 500 N/m, 0.15 / 0.16 m) and a preload computed for those, so the soft worn setup
from my round-12b retune was gone. Running `build_roster.py` would have wiped A's work on
every car the other way round.

**Fix in the source of truth (`tools/specsheet/`):** `build_roster.py` now applies A's
round-12b rules (section "Round 12b (A)" below): derived dampers × 1.2 bump / × 1.6 rebound,
handbrake ≥ 1.5 × the static rear lock torque (`handbrake_nm`), and preload may be a
`(front, rear)` pair in `roster_inputs.py` (A's measured values on drayman_cargo, furrow_24,
ironside_440, tansy_13). Regenerating now reproduces A's data **byte-for-byte on all 18 other
cars**. A's rules are mirrored in the spec sheets and `VALIDATION.md`, as A asked.

**Beater** (`roster_inputs.py`), checked with `cargo run --release -p wp_vehicle --example ride all`:

| Change | Why |
|---|---|
| ζ 0.14 → **0.18** (→ 0.21 bump / 0.42 rebound after A's scale) | `tests/ride.rs` floor ζ_bump ≥ 0.2; at 0.14 it flew 0.49 s and rose 0.60 m off the 15 cm kerb at 60 km/h. Still the softest dampers in the roster. |
| preload 0.01 → **0.11 F / 0.08 R** | it sat on its front bump stops (0 mm free); now 72 / 80 mm = 45 % / 47 % of travel |
| camber −1.0 / −0.5 → **−1.3 / 0°**, rear toe +0 → **−0.15°** (toe-out), roll split 0.60 → **0.70** (rear bar 0: many 80s economy front-drivers had none) | understeer back to ~1.5 °/g; camber alone moved < 0.05, toe-out −0.08, roll split −0.06 |

Result: **1.52 °/g** (band 0.25–1.6; still the most understeery front-driver), skidpad 0.69 g
(skidpad gradient +0.08 °/g: no limit oversteer), 0-100 **15.5 s**, Vmax 146 km/h, 100-0 46.5 m,
roll 6.3 °/g, barriers 8/8. Ride: never airborne on the 10 or 15 cm kerb at 60 km/h (rise 0.088 m),
no lift on the rounded crest. `wp_vehicle` `ride`, `handling_balance`, `slide_control`,
`differentials` pass; nameplate, `smoke_sim`, `pursuit_probe` (cruiser catches it in 5.0 s),
3 `cars/` scenarios pass. `[bless]`: `roster_diff_hashes_are_pinned` junkyard_beater
`0x72a8eef4cb8c2001 → 0xbe54b8910b0c86e8` (others unchanged) and the three `cars_beater_*` goldens.

**Gate (ws/cars `1af33b4` + this doc):** clippy `-D warnings` ✓, wasm build ✓. `cargo fmt --check`
fails only on `crates/wp_vehicle/src/lib.rs` (`pub mod ride;` sorted after `presets`), identical
on `7a2f19b` (A / lead; not touched here). `cargo test --workspace --no-fail-fast`: 145 binaries
ok; every failure also fails with the base's own beater file (`7a2f19b`) swapped in:
`wp_audio rev_matched_downshifts_blip` (J), `wp_race race_picker::{loop9_circuit,
lantern_checkpoint_draws_the_police}` (L / N), `wp_wasm crash_replay_tests::race_crash_replays_…`
and `tests::simcli_golden_replays_play_back_in_the_browser_core`, and `simcli goldens_match`:
standalone debug `simcli goldens` lists the **same 49 failures** with either beater, none of
them `cars_*` (crash, damage, panels, golden, city, race, police, weather: the round-12b ride
work needs a lead re-bless).

**For A:** the ride rules now live in `build_roster.py`. Next time, change `roster_inputs.py`
or the constants there (`RIDE_BUMP_SCALE`, `RIDE_REBOUND_SCALE`, `HANDBRAKE_LOCK_MARGIN`)
instead of the JSON, or tell me and I regenerate.

## Lead follow-up round 12b (2026-10-20) — beater understeer gradient

Gate on `lead/round12`: `wp_vehicle/tests/handling_balance.rs::front_drive_cars_understeer_mildly`
measured the retuned beater at **1.71 °/g** (band 0.25-1.6). Merged `origin/lead/round12` at
`c982c82` (fast-forward). **Retuned the car; the test band is unchanged.**

What the gradient responds to (ramp steer, wheel preset, 80 km/h): almost only tyre cornering
stiffness × axle load. Roll-stiffness split 0.40 / 0.50 / 0.60 / 0.70 gave 1.65 / 1.73 / 1.71 /
1.61 °/g (no use); newer `street` tyres on the front only gave −2.6 °/g of *limit oversteer* on
the skidpad (rejected: not readable). What went in (`roster_inputs.py`):

| Change | Why it is honest | Effect |
|---|---|---|
| front weight 0.62 → **0.60** | small 1.6 FF four-door with a long boot; 60/40 is mid-class | main lever |
| front camber −0.2° → **−1.0°** | sagging MacPherson struts gain negative camber | 1.71 → 1.64 alone |
| rear toe-in 0.10° → **0** | worn rear bushes | small |

Result: **1.50 °/g** (still the most understeery front-driver: Vireo 0.46, Kestrel 0.35), skidpad
0.72 g, 0-100 14.7 s, Vmax 146 km/h, 100-0 45.0 m, barriers 8/8; tyres (`vintage_bias`), soft
springs, ζ 0.14 and 6.5 °/g roll unchanged, so it still feels tired. Lattice unchanged (panels fit).
Re-recorded in a `[bless]` commit: the beater's pinned hash in
`wp_vehicle/tests/differentials.rs` (→ `0xc8ab6690a927e4c2`) and the three `cars_beater_*`
goldens. Note for the helper: the comment in `front_drive_cars_understeer_mildly` still says
"the junkyard beater 1.42"; it is 1.50 now.

**Correction (2026-10-20): there is no debug/release hash split.** An earlier version of this
section reported that steering scenarios hash differently in debug and release builds. That was
my measuring error: the release `simcli` binary was stale, built before the `c982c82` merge,
while the debug one was current. After `cargo build --release -p simcli` on the same commit,
`vehicle/coupe_slalom`, `smoke/coupe_circle` and all three `cars_beater_*` scenarios hash
identically in both builds, and release `simcli goldens` fails on exactly the same 33 base
scenarios as debug (none of them `cars_*`). The `cars_beater_hard_turn` golden is valid for
both builds and CI's release goldens job.
*Lead (2026-10-20, `lead/det-debug-release`): resolved — there is no split; the release
binary was stale (built before the handling-balance merge). Fresh debug and release builds hash
identically and `cars_beater_hard_turn` passes in both. See harness.md "Lead helper: debug vs
release hashes".*

**Checks (follow-up, cloud container):** fmt ✓, clippy `-D warnings` ✓, wasm build ✓,
`handling_balance` 6/6 ✓, `differentials` ✓, nameplate (69 files, 0) ✓, `smoke_sim` ✓,
`collect_results` / `pursuit_probe` ✓, my 3 scenarios ✓. `cargo test --workspace --no-fail-fast`:
remaining failures are all on `lead/round12` `c982c82` — each re-run with the pre-retune beater
files swapped back in gives the same result: `wp_race::race_picker::grid_highway_sprint`,
`wp_sim::sidewalk::vesper_v2_cars_drive_up_the_kerb_and_along_the_sidewalk`,
`wp_wasm::race_tests::{short_sprint_with_ai_runs_to_results_and_pays_through_the_economy,
a_capture_mid_race_is_a_dnf_and_hands_off_to_the_end_of_run_panel}` and `simcli`
`goldens_match` / `all_scenarios_…` (debug `simcli goldens`: the same 33 failures with old and
new beater data, none of them a `cars_*` scenario; e.g. `city_sidewalk_mount_*` `min_z_m` 2.37 vs
2.47-2.53, `blocked_parked_player_vesper` turnarounds 0, race picks, weather, police sideswipes).

## Response to lead feedback round 12b (2026-10-19) — junkyard beater (B06)
Merged `origin/lead/round12` at `05f92f8` into `ws/cars` (not main, per the brief); no conflicts.
`junkyard_beater` already existed (round 6b, as the economy floor) but missed the new brief
(sim 0-100 13.8 s, Vmax 164 km/h, 5-speed, new `street` tyres). Retuned in `roster_inputs.py`
and regenerated (car JSON, spec sheet, lattice), validated, and given scenarios.
| Item | Done |
|---|---|
| 1 Car file | `data/cars/junkyard_beater.json`: 1980s FF 1.6 SOHC carb four-door, **1048 kg** (curb 990), **65 kW @ 5500 / 124 N·m @ 4000** (worn, soft midrange), redline 5800 / limiter 6000, **4-speed manual** (3.17 / 1.84 / 1.26 / 0.97, final 4.33, 0.55 s shift), **open diff**, worn drivetrain `efficiency` 0.78. **Soft worn suspension**: ride 1.15 / 1.25 Hz, ζ 0.14, roll gradient 6.5 °/g, +1 cm travel. **Old tyres**: `vintage_bias` (≈ 0.85 × street grip, strong fall-off past the peak = early, long squeal; see request to A + G for a proper aged-radial compound). Durability 2, base price 3 000 (= `market.toml [floor_car]`, floor price 1 500 untouched; `roster_prices::floor_car_is_in_the_roster_with_matching_data` passes). The 3-speed automatic: one drivetrain per `VehicleDef`, so the car ships as the 4-speed manual and the auto is a request to G (below). Lore line in `docs/design/CARS_LORE.md` (no longer "proposed"/"no car file"). Nameplate check clean. |
| 2 Lattice + panel mesh | Reused the existing generated sedan body: `gen-lattice --profile sedan`, same dims/strength 0.9. Only node masses changed (car 10 kg lighter): node positions, beams, detachables and skin binding are identical, so R's `data/panels/junkyard_beater.json` and its mesh fit unchanged — no new mesh needed. |
| 3 Validation band, tests, testbed | `VALIDATION.md` band: 0-100 **14-16 s**, skidpad 0.68-0.78 g, 100-0 42-48 m and a new optional **top-speed band 140-160 km/h** (`specsheet.py`, only the beater uses it). Sim: **0-100 14.7 s ✓, Vmax 146 km/h ✓, 100-0 45.5 m ✓, skidpad 0.72 g ✓**, mild understeer +0.19 °/g; barriers 8/8. Tests: new scenario group `apps/simcli/scenarios/cars/` (`beater_accel` 0-100 between 14 and 16 s, `beater_top_speed` 140-160 km/h, `beater_hard_turn` full lock at 60 km/h: 0.5-0.85 g, never airborne, keeps moving); they run in `cargo test -p simcli`'s scenario suite. **Testbed car list**: nothing to add — `wp_wasm` embeds every `data/cars/*.json` (`build.rs`) and the car dropdown is built from `car_ids()`; `wp_wasm`'s `every_embedded_car_resolves` covers it. |
Pursuit probe (`pursuit_probe.py --chase`): beater 0-144 km/h 35.5 s; the cruiser catches it in
5.1 s at the day cap (12.6 s with the +10 % margin). Validation re-collected on the round-12
base: the other cars moved only at rounding level (A's round-12 vehicle changes), barrier sweep
unchanged (the known Wolfhound offset near miss).
Tool changes (`tools/specsheet/`, mine): `build_roster.py` writes an optional
`drivetrain.efficiency` (and explains it in the spec sheet) when a car states one;
`estimate.py` uses it; `specsheet.py` checks the optional top-speed band.
### Changes outside my folders (round 12b)
- `docs/design/CARS_LORE.md` (P's design docs): the beater entry — the lead asked for the lore line.
- `apps/simcli/scenarios/cars/` (new group, I's harness folder; CLAUDE.md puts scenario tests there)
  and their goldens `apps/simcli/golden/cars_beater_*` (new only, `[bless]` commit).
- `crates/wp_vehicle/tests/differentials.rs` (A): `roster_diff_hashes_are_pinned` pins a
  `junkyard_beater` hash; re-recorded `0xe9ef92e8d452f551 → 0xb565bf983dfcffda` in a `[bless]`
  commit (the test says to re-record on a car-data change). The other five cars are unchanged.
**Checks (round 12b, cloud container):** `cargo fmt --check` ✓, `cargo clippy --workspace -- -D
warnings` ✓, `cargo build --target wasm32-unknown-unknown -p wp_wasm` ✓, `nameplate_check.py`
(69 files, 0 problems) ✓, `smoke_sim.py` (17 cars × 2 modes deterministic) ✓,
`collect_results.py` / `pursuit_probe.py --chase` ✓, my 3 `cars/` scenarios ✓.
`cargo test --workspace --no-fail-fast`: everything passes **except failures that are already on
`origin/lead/round12` (05f92f8)**. I proved each one independent of my change by running it with
the same binary and the old beater files swapped back in: identical result.
- `simcli` `scenarios::all_scenarios_pass_and_are_deterministic_in_process`:
  `city/blocked_parked_player_vesper` expects `traffic_turnarounds` ≥ 1, gets 0 (debug_sedan,
  D's blocked-vehicle traffic work, `c29eeca` / `7f1a3ea`).
- `simcli` `scenarios::goldens_match`: hash mismatches on `city_blocked_parked_player_{grid,vesper}`,
  `golden_city_chase`, `golden_night_pursuit_escape`, `panels_frontal_56_hood`,
  `police_sideswipe_patrol_day`, `weather_{aquaplane_control,aquaplane_puddle,fog_pursuit,
  fog_pursuit_clear,wet_brake_100_0}` (debug build; the release `simcli goldens` lists 18, the same
  18 with the old data). None of them spawns the beater. → lead: re-bless after the round-12 merges.
- `wp_net` `vesper_e2e::four_clients_three_minutes_in_port_vesper`: p0 `Escaped`, expected
  `Captured` (debug_coupe racer; same with the old data). → F / lead.
### New requests (round 12b)
| To | Request |
|---|---|
| 02 vehicle (A) + 08 economy (G) | An **`aged_radial`** compound for old, hardened radials (the beater; later traffic/props). `vintage_bias` stands in today but is a 70s bias-ply (half the cornering stiffness, lazy). Proposal: street's shape with lateral `d` 0.82, `e` −0.5 (earlier, louder fall-off), longitudinal `d` 0.95, relaxation 0.40 m, rolling resistance 0.014, wet 0.55, paint 0.93 (peak slips computed from the curves). It needs a `market.tire_grip` row too (`wp_economy`'s `every_tire_file_has_a_grip_factor`), which is why I did not add the file. I switch the beater with one line in `roster_inputs.py` once it exists. |
| 08 economy (G) | A **3-speed automatic** option for the beater (the brief's "4-speed manual / 3-speed auto"): a gearbox-swap mod, or a second def `junkyard_beater_auto` if you prefer a used-market variant. Data for it: ratios 2.84 / 1.60 / 1.00, final 3.70, torque converter stall 2200 rpm ×2.0, couples 2600 rpm, shift 0.6 s — I add the def on request. |
| 19 design docs (P) | `FEATURE_BACKLOG.md` B06 can be closed. |

## Vehicle-models wave 1: roster agent (2026-10-21)

Plan: `docs/design/vehicles/PLAN.md` (roster row). Three new cars and multi-model police /
traffic selection.

### New cars (all from `roster_inputs.py`; full sim validation in `VALIDATION.md`)

| Id | Class | Mass (sim) | Power | 0-100 sim (est.) | Vmax sim | 100-0 | Skidpad | Dur. | Price |
|---|---|---|---|---|---|---|---|---|---|
| `fulmine_v10` | Exotic | 1634 kg | 449 kW (5.2 NA V10, 8250 rpm) | 3.59 s (3.5) | 337 km/h | 34.9 m | 0.95 g | 3 | 125 000 |
| `sentinel_warden` | PoliceCruiser | 1873 kg | 155 kW (5.7 TBI pushrod V8) | 8.80 s (8.6) | 202 km/h | 40.1 m | 0.81 g | 4 | 26 000 (not for sale) |
| `sentinel_harrier` | PoliceInterceptor | 1813 kg | 319 kW (5.0 DOHC V8) | 5.74 s (5.7) | 275 km/h | 38.2 m | 0.87 g | 4 | 52 000 (not for sale) |

- Dimensions follow the wave-1 research (`research_performance.md` §2, `research_police.md`
  §1): Fulmine 1.95 × 4.46 × 1.17, wheelbase 2.65, 245/35R20 (loaded r 0.330) / 305/30R21
  (0.347); Warden 1.98 × 5.40 × 1.43, wb 2.90, front overhang 1.05, 225/70R15, durability 4;
  Harrier 1.93 × 4.82 × 1.37, wb 2.75, 275/40R20 square. Harrier kept durability 4 (research
  says 3) because it carries the reinforced push bar + doubled bumper beam like the other
  pursuit units.
- Fulmine: fixed `Awd { front_split: 0.30 }`, 7-speed dual-clutch (0.06 s shifts), clutch
  rear LSD. Durability 3 (not 2): at strength 0.9 the 56 km/h frontal crushed 1.32 m (band
  0.35-1.3); a bonded aluminium/CFRP cell is a modern safety structure.
- Warden: body-on-frame sedan = gen-lattice `cruiser` profile + `ladder_frame` (new
  `TRUCK_RAIL_SCALE["sedan"] = 1.15`, a perimeter frame lighter than a truck ladder), 80s
  passenger cell (hss × 0.65), drum rears, police package like the cruiser.
- Barrier matrix: all pass except Harrier `offset 40% 56 km/h` (struck-side corner intact,
  total loss) — the same known result as the Wolfhound (push-bar front spreads the offset load).
- Market: prices come from the car files (`wp_economy` market); the Fulmine sits in the
  Exotic band (60 000-320 000) in every condition, its impound base fee is 2.0 % of value, its
  class index (832) tops the roster. Police classes are refused by `buy_car`.
  **Price 125 000, not the ~230 000 the class would cost:** `wp_economy`'s
  `roster_table_and_feel_targets` requires a medium front hit ≤ 1.6 × the class win purse in
  Simulation; the Exotic purse (7 452) is shared with the 120 000 Lucciola and repair prices
  scale with value, so 230 000 gave 2.73. **Request to 08 (economy):** a separate purse tier
  for class-index ≥ 800 cars (or a value-scaled Exotic purse) so the Fulmine can be priced at
  ~230 000; then change `price` in `roster_inputs.py`.
- Audio: `data/audio/cars/{fulmine_v10,sentinel_warden,sentinel_harrier}.json` (audio's
  folder; `wp_audio` requires one per car). V10 uses the V8 pack (no V10 pack yet), 10
  cylinders, even firing.
- Panels / meshes: `gen-panels` + `gen-car-mesh` for the three ids only.
- **Cab-forward exotic lattice not done:** gen-lattice places the cabin by profile (`coupe`
  shape fractions), with no knob for cabin position, so `fulmine_v10` (like `lucciola_350`)
  has a front-engine coupe cabin. Needs either a `mid`/`cab_forward` profile in
  `wp_damage::lattice_gen` (03) or the foundation's visual `cabin_shift` (research option b).

### Multi-model police (`wp_sim`)

```rust
pub struct PoliceModel { pub car: LoadedCar, pub min_level: u8, pub max_level: u8,
                         pub weight: u32, pub limits: Option<wp_police::pro::CarLimits> }
impl PoliceModel { fn new(car, min_level, max_level) -> Self; fn with_weight(self, u32) -> Self;
                   fn with_limits(self, CarLimits) -> Self }
pub fn pick_police_model(fleet: &[PoliceModel], level: u8, seed: u64, unit: UnitId) -> usize;
impl Sim {
    pub fn set_police_cars(&mut self, fleet: Vec<PoliceModel>);   // empty = back to set_police_car
    pub fn police_fleet(&self) -> &[PoliceModel];
    pub fn police_unit_model(&self, unit: UnitId) -> Option<usize>;
    pub fn police_dispatch_level(&self) -> u8;                     // max racer wanted level 0..=5
}
```

- Each AI car unit gets a model **once**, the first reconcile after it appears (in
  `wp_police` unit order), at the current dispatch level (max wanted level of any racer,
  level 0 counted as 1), by `SplitMix64(seed ^ salt ^ (unit_id << 8 | level)) % total_weight`
  over the models whose `min_level..=max_level` contains the level (all weighted models when
  none does). Stored in a `BTreeMap<UnitId, usize>`, pruned when units retire; no RNG draws,
  not part of `state_hash` (the spawned cars are). Kinematic units keep their model when they
  later get a body.
- `set_police_car` is untouched; with no fleet the old path runs unchanged. A one-model fleet
  hashes identically to `set_police_car` (test `a_one_model_fleet_matches_set_police_car`).
- Per-model `limits` override `set_police_car_limits` for that unit.

### Multi-model traffic (`wp_world` + `wp_sim`)

```rust
pub fn wp_world::traffic::traffic_model_index(id: AgentId, n: usize) -> usize; // SplitMix64(id) % n
impl Traffic { fn set_traffic_cars(&mut self, Vec<(VehicleDef, TireDb)>); fn model_count(&self) -> usize;
               fn model_of(&self, AgentId) -> usize }
impl World { pub fn set_traffic_cars(&mut self, &[LoadedCar]); pub fn traffic_model_of(&self, AgentId) -> usize }
impl Sim   { pub fn set_traffic_cars(&mut self, Vec<LoadedCar>); pub fn traffic_cars(&self) -> &[LoadedCar] }
```

- Derivation per agent: **id** = `wp_world`'s stable agent id (spawn order, from 1);
  **model** = `traffic_model_index(id, n)` (pure function of the id; pinned by a unit test so
  renderers can rely on it); **variant** = the existing colour byte drawn once at spawn
  (`rng.below(8)`), never touched by promotion. So a car never changes model or colour when it
  becomes physical, and the instanced (kinematic) renderer can draw the same model.
- One model: identical to `set_traffic_car` (test `a_one_model_list_matches_set_traffic_car`;
  the world's `cars` is now a `Vec`, `set_traffic_car` sets a one-element list).

### simcli + browser wiring

- New scenario field `car_mix: bool` (default false, omitted when false — existing scenarios,
  goldens and replays unchanged). True: police fleet `simcli::run::CAR_MIX_POLICE` (warden 1–2,
  cruiser 1–2, bastion 2–3, wolfhound 3–5, harrier 4–5, limits from VALIDATION.md; cruiser on
  `pursuit.toml` defaults) and traffic `CAR_MIX_TRAFFIC` (index → id: 0 `debug_sedan`,
  1 `junkyard_beater`, 2 `tansy_13`, 3 `furrow_24`, 4 `hauler_1500`, 5 `drayman_cargo`,
  6 `vireo_xs`).
- `wp_wasm::scenario::city_chase_in` sets `car_mix = true`. Streams (see `testbed.md`):
  `traffic_agents()` stride 5 → 7 (`agent_id`, `model`), `police_units()` stride 6 → 8
  (`unit_id`, `model`), new `traffic_models()` / `police_models()` (JSON car-id tables),
  `vehicle_meta()` flags carry physical traffic's `variant << 8`. `apps/testbed-web`: strides,
  `AG` / `PU` offsets in `layout.ts`, `sim.trafficModels()` / `sim.policeModels()`; e2e
  divisors updated. Rendering of the models is wave 2's job.
- With heat at L0–L1 only warden/cruiser patrol; bastion appears from L2, wolfhound from L3,
  harrier at L4–L5, so all five show up as a chase escalates.

### Changes outside my crates (this round)

- `crates/wp_sim/src/lib.rs` (lead): three fields (`police_fleet`, `police_unit_models`,
  `traffic_cars` replacing `traffic_car`) and the `PoliceModel` re-export.
- `apps/simcli/src/scenario.rs`, `fuzz.rs`, `examples/city_frame_cost.rs`, `SCENARIOS.md`
  (harness): the `car_mix` field.
- `data/audio/cars/*` (audio) for the new cars; `apps/testbed-web/src/{scene.ts,wasm/*}` and
  `e2e/run.mjs` (testbed): parsing only.
- `tools/specsheet/*.py` now honour `CARGO_TARGET_DIR` for the release binaries.

## Response to lead feedback round 8 (2026-10-19)

Merged `origin/main` at `d3effad` (contract v0.3.0) into `ws/cars`; no conflicts.

| Item | Done |
|---|---|
| Lead's changes to my civilian cars | **Kept, and moved into the source of truth** (`roster_inputs.py`): `tansy_13` 7 000 + `shift_time_s` 0.25, `drayman_cargo` 20 000, `furrow_24` 21 000. `build_roster.py` now regenerates byte-identical car JSON and lattices to `main`; spec sheets / ROSTER.md show the new prices. Re-run with the new shift: Tansy 0-100 13.3 s (band 11.5-13.5), unchanged within 0.1 s. Panel meshes (`data/car-assets`, R) not touched. |
| 1 `semi_slick.json` `treaded: false` (W-4) | Done. Every other compound stays treaded (`None`), the drag radial too (DOT street tire with shallow tread). |
| 2 Tire data for `PaintedLine` | Done on all 8 compounds: **dry** tire-on-paint multiplier 0.92-0.96 (street / police 0.95, wet-biased 0.96, sport / all-terrain 0.94, vintage bias 0.93, semi-slick / drag radial 0.92). Paint fills the asphalt macrotexture, so the hysteresis share of grip drops; compounds that rely on it most lose most. Reasoning + a `paint` column in `data/tires/README.md`. **Wet** paint grip stays in the conditions model (see request below: do not stack a second *dry* paint factor). |
| v0.3.0 checklist: push bar → `Accessory` | Done in `build_roster.py` (+ R's generated panel specs/meshes for the three cars regenerated, see "Changes outside my crates"): the 4 push-bar nodes and detach group 8 of `sentinel_cruiser`, `sentinel_wolfhound`, `sentinel_bastion` are `BodyRegion::Accessory` (was `FrontBumper`). `ram-probe` on all three, both modes, 4 ram kinds × 3 speeds × 6 rams: **health, disengage ram and total-loss ram identical** to before; barrier verdicts identical. Only the crush *metrics* changed (request to B below). |
| Validation re-collected on `main` | `collect_results.py`, `specsheet.py`, `nameplate_check.py` (69 files, 0 problems), `smoke_sim.py` (17 cars × 2 modes deterministic), `pursuit_probe.py --chase`, `ram-probe`. Main's vehicle changes (A's faster pedal filter `880753b`, save-the-car steering) shortened stops ≈ 1-2 m roster-wide and moved two cars across a band edge: **Vireo 100-0 37.9 m** (band 38-44) and **Lindwurm 0-100 5.42 s** (band ≤ 5.4). Both explained in VALIDATION.md, not retuned (rounding-level, caused by the vehicle model). Cruiser now 0-100 6.4 s, 0-144 11.65 s; it still reaches the day pursuit cap before every traffic and cheap car. Barrier sweep unchanged: 135/136, the one miss is the known Wolfhound offset near miss. |

**Checks (round 8, cloud container):** `cargo fmt --check`, `cargo clippy --workspace -- -D
warnings`, `cargo build --target wasm32-unknown-unknown -p wp_wasm`, `cargo test --workspace`
(result below), plus the tool chain above.

### New requests (round 8)

| To | Request |
|---|---|
| 03 damage (B) | `DamageModel::structural_crush_m` and `max_displacement_along` skip glass and bumpers but not `Accessory`, so a crushed push bar now reads as structural crush (cruiser frontal 25 km/h 0.24 → 0.44 m; health unchanged). Add `BodyRegion::Accessory` to both exclusion lists (2 lines). Also: a detached `Accessory` group adds the generic 0.04 drag in `handling_effects`; 0 or the bumper 0.03 fits a push bar better. |
| 08 economy (G) | `wp_economy::data::ALL_REGIONS` (20) and `data/economy/prices.toml` have no `Accessory` row, so a lost or bent push bar bills 0. Suggest ≈ 350 (bar) with the bumper's labour factor. |
| lead (`wp_wasm`) | `wp_wasm::pack::ALL_REGIONS` (20) has no `Accessory`; `region_index` falls back to 0, so push-bar damage shows as front-bumper damage in the testbed. Harmless today; append it when the region arrays are next touched. |
| 02 vehicle (A) + 14 weather (N) | `surface_multipliers[PaintedLine]` is now the **dry** tire-on-paint grip (0.92-0.96). When a wheel's surface *is* `PaintedLine`, do not also apply `PAINT_DRY_FACTOR` (0.95, `wp_vehicle::surface`) / `marking_dry_factor` (0.97, `surfaces.toml`), or dry paint is counted twice. The wet loss (`PAINT_WET_FACTOR`, `marking_wet_factor`, or a `PaintedLine` `wet_loss_scale` ≈ 1.6) stays with you. The `painted_line` flag on a non-paint surface is unaffected. |

## Response to lead feedback round 6b (2026-10-19)

| Item | Done |
|---|---|
| 1 Cruiser pursuit tuning | **V8 pursuit package**: `sentinel_cruiser` is now a 5.7 L pushrod V8 (276 kW, 535 N·m, 2008 kg incl. driver/half tank). Sim: 0-100 **8.4 → 6.8 s**, 0-144 km/h (the day pursuit cap) **16.2 → 12.5 s**, 80-144 **10.1 → 7.2 s**, Vmax 260, 100-0 40.0 m, skidpad 0.83 g. It now reaches the cap before every Beater/Compact/Truck/70s-muscle car and the FF sport sedan, and close behind the sport cars (table below). Durability: superseded by the police package (section "Lead follow-up: cop cars too fragile"). |
| 2 Traffic variety | 3 civilian cars, full pipeline (VehicleDef + spec sheet + lattice + audio def, validated, smoke-simmed): **`tansy_13`** Tansy 1.3 — 2000s FF 1.3 supermini hatch (Compact, 4 500); **`drayman_cargo`** Drayman Cargo — 2000s RWD 4.8 V8 full-size cargo van, body-on-frame (Truck, 9 000); **`furrow_24`** Furrow 2.4 — 90s RWD 2.4 I4 compact pickup, body-on-frame (Truck, 6 500). Sim: 13.2 / 11.6 / 10.8 s 0-100, 0.79 / 0.66 / 0.73 g, 39.9 / 43.7 / 41.4 m, 168 / 201 / 178 km/h — all in their class bands except van and pickup stopping 0.3-0.6 m *shorter* than their bands (car-type `street` tires on light trucks). Barriers 8/8 each. Nameplate check clean. |
| 3 `gen-lattice` pass | All 17 roster lattices regenerated on B's current generator (`ab6970d`: rail bracing, side attachments). **Folded back what I can without touching `wp_damage`**: every remaining hand adjustment is now listed with the exact `gen-lattice` option it should become (section "Proposed gen-lattice options" below, ready to implement 1:1). Two new adjustments fix generator findings in car data: the **luggage bay / cargo box** behind the seatback is no longer passenger cell (fixes the two-box rear total loss on Vireo, Lindwurm, Bastion and the new Tansy — 4 barrier misses gone), and **mid-engine front rails** get the rear-rail factor (Lucciola 56 km/h crush 1.47 → 1.26 m). Barrier sweep: **134/136** (2 near misses: police offset, below). |

**Checks (round 6b, cloud container):** fmt, clippy `-D warnings`, wasm build of `wp_wasm` (the
new tool crate is not reachable from it), `nameplate_check.py` (68 files, 0 problems),
`smoke_sim.py` (17 cars × 2 modes deterministic), `collect_results.py`. `cargo test --workspace`
(run with `--release`: the debug simcli scenario suite exceeded the 30-min tool limit): **all
pass** after merging the lead branch at `f92bf16` (the two failures seen earlier,
`pursuit_wiring::real_police_capture_settles_through_the_economy` and
`simcli_races::city_sprint_on_real_roads`, failed on the base branch too and are fixed there).
**8 city goldens
re-blessed** in separate `[bless]` commits (V8 cruiser, then the police package): simcli spawns `sentinel_cruiser` as the city police
car, so every city run with police changed; with the old data all 35 goldens match, all 8 still
pass their expectations.

### Pursuit numbers (`python3 tools/specsheet/pursuit_probe.py --chase`, full sim, Arcade)

| car | 0-100 s | 0-144 s | 80-144 s | caught (+10 % margin, day cap) | caught (cap only) |
|---|---|---|---|---|---|
| **sentinel_cruiser** (police package) | **6.97** | **12.75** | **7.38** | — | — |
| sentinel_wolfhound | 6.48 | 11.98 | 7.63 | no | no |
| tansy_13 / junkyard_beater | 13.2 / 13.8 | 30.9 / 34.4 | 22.3 / 25.1 | 12.3 s / 12.5 s | 6.4 s / 6.2 s |
| drayman_cargo / furrow_24 | 11.6 / 10.8 | 24.0 / 24.4 | 16.1 / 17.0 | 12.2 s / 12.1 s | 7.0 s / 7.1 s |
| vireo_xs | 9.20 | 18.12 | 11.63 | 11.4 s | 8.7 s |
| debug_sedan (traffic) | 8.13 | 15.17 | 9.32 | no | no |
| hauler_1500 / ironside_440 | 8.98 / 8.72 | 16.65 / 15.15 | 10.10 / 8.35 | no | no |
| kestrel_ts | 7.15 | 13.02 | 7.92 | no | no |
| sport cars / exotic | 4.6-6.3 | 8.2-11.5 | 4.6-6.7 | no | no |

Chase model: target 30 m ahead, both leave a corner at 15 m/s at full throttle, caught if the
gap closes before the target has covered 400 m. **What stops the cruiser catching the traffic
sedan, the Hauler, the Ironside or the Kestrel on a straight is `wp_police`'s speed limit, not
the car**: pursuit speed is `min(target × 1.10, cap)` with a 40 m/s day cap (32 at night), and
every car except the Beater tops out above 150 km/h, so a full-throttle target pulls away once
the cop sits on the cap. With a 50 m/s cap and 800 m the current cruiser also catches the
traffic sedan, Hauler and Ironside (measured with the same model); the Kestrel and every sport
car still need the Wolfhound. Making the cruiser itself catch those would make it faster than
the interceptor. → request to E/lead below.

### Durability under repeated hits — superseded by the police package below

The first round-6b numbers (damage-model `ram-probe` only) are replaced by the full-sim
sequence in "Lead follow-up: cop cars too fragile".

## Lead follow-up: "cop cars too fragile" (playtest row 12, 2026-10-19)

**`sentinel_cruiser` police package** (all in `roster_inputs.py` → car JSON + lattice):

| What | Change |
|---|---|
| Durability class | 4 → **5** (`gen-lattice --strength` 1.15 → 1.30: every structural section +13 %) |
| Mass | curb 1960 → 2000 kg (+40 kg: heavier push bar, HD cooling/suspension), front share 0.54 → 0.55 — the extra mass sits on the nose |
| Lattice | 25 % of vehicle mass (was 22 %): reinforced structure + push bar |
| Push bar | 25 → 40 kg, tube and mount sections × 1.5, detach impulse 15 000 → 30 000 N·s (it bends before it comes off) |
| Front bumper fixings | detach impulse × 2 |
| Heavy-duty components | failure intrusion × 1.5 radiator (HD cooling), steering rack, front suspension; × 1.4 front wheel mounts; × 1.3 front brakes, battery; × 1.2 engine |
| HD suspension | ride 1.45 / 1.55 Hz (was 1.35 / 1.45), ζ 0.32, roll gradient 3.2 °/g, front roll share 0.57, front camber −1.0° — keeps the heavier nose from pushing |

Handling stays credible: 0-100 6.97 s (band 6.2-7.2), 100-0 40.1 m, skidpad 0.85 g (band
0.85-0.92), 0-144 km/h 12.75 s (was 12.5). Barrier sweep 8/8 for the cruiser (the offset near
miss is gone; frontal 56 km/h is no longer a Simulation total loss, which the sweep allows for
durability ≥ 4).

**Proof — full-sim ram sequence** (`python3 tools/specsheet/ram_sequence.py --cop <car>`):
the police car is parked (handbrake) and 1400 kg strikers (`debug_sedan`) hit it one after
another through `simcli` (Rapier contacts → damage, exactly the game's path), each on its own
approach line, timed 8 s apart, aimed at where the car actually is after the previous hits;
every hit is confirmed from `events.jsonl`. A striker into the parked cop's nose at v loads the
cop like the cop ramming a stopped car at v; the corner hits are the cop's side of a PIT.
Health 1.5 s after each hit:

| # | hit (closing speed measured) | Cruiser Arcade | Cruiser Sim | civilian sedan Arcade | civilian sedan Sim |
|---|---|---|---|---|---|
| 1 | ram, nose, 54 km/h | 0.96 | 0.87 | 0.95 | 0.81 |
| 2 | PIT, left front corner, 59 km/h | 0.90 | 0.68 | 0.83 | 0.42 |
| 3 | PIT, right front corner, 64 km/h | 0.81 | 0.48 | 0.70 | 0.12 **TL** |
| 4 | ram, nose, 69 km/h | 0.74 | 0.34 | 0.61 | **TL** |
| 5 | PIT, left front corner, 59 km/h | 0.66 | 0.20 **TL** | 0.47 | **TL** |
| 6 | hard hit, right side (T-bone), 84 km/h | **0.42** | TL | 0.22 | TL |
| 7 | hard hit, rear, 84 km/h | 0.33 | TL | 0.21 | TL |

- **Arcade: meets the bar.** 5 rams/PITs at 55-70 km/h and 2 hard hits at 85 km/h, never a
  total loss. It stays in the pursuit (health above `wp_police`'s disengage 0.4) through all 5
  rams and the first hard hit; the second hard hit retires it (0.33). Before the package (same
  run, previous cruiser data): 0.63 after the 5 rams and **0.30 after the T-bone** — retired by
  the first hard hit, like the civilian sedan (0.22).
- **Simulation: clearly tougher than the civilian sedan, still realistic.** Total loss on the 5th
  hit (after 4 rams/PITs at 55-70 km/h); before the package on the 4th (out of the chase after
  the 3rd); the sedan is a total loss on the 3rd. One 85 km/h
  T-bone on its own still ends either car.
- Arcade never reaches `TotalLoss` for *any* car here: Arcade health levels off ≈ 0.2-0.3 (the
  0.3 component share keeps it there) and the Arcade total-loss health is 0.1. So in Arcade the
  gameplay limit is `wp_police`'s disengage health (0.4), not the damage model — see Needs.
- Damage-model cross-check (`./target/release/ram-probe sentinel_cruiser`, 6 rams each, Arcade):
  frontal push-bar rams 60 km/h 0.95 → 0.74, 80 km/h 0.90 → 0.51; side hits taken 80 km/h
  0.77 / 0.55 / 0.39 (3rd retires it).

## Proposed gen-lattice options (for 03 / B — what `adjust_lattice` still does by hand)

Each is a self-contained step in `tools/specsheet/build_roster.py::adjust_lattice` with the
exact node/beam selection; each car's spec sheet lists what was applied. Proposed flag → what
it does → cars:

| Proposed flag / `LatticeParams` field | Hand adjustment it replaces | Cars |
|---|---|---|
| `--era 70s\|80s\|90s\|2000s\|2010s` | Passenger-cell members (cabin, sills, pillars, roof rails, cabin cross-members): 70s mild steel × 1.0 (+ door beams × 0.5), 80s HSS × 0.65, 90s × 0.75, 2000s × 0.90, 2010s as generated. Needs the generator's beam roles — today matched by section area (`SEC` mirror, fragile). | all |
| `--cabin-end seatback\|partition` (or fix in the `hatch`/`suv` profiles) | Cabin nodes behind the rear seatback (hatch: rear axle + 0.10 m, SUV: − 0.10 m) or behind the front-seat partition (van) relabelled `FrameRear`. **New this round**; without it a 40 km/h rear hit totals every two-box car. | Vireo, Lindwurm, Bastion, Tansy (seatback); Drayman (partition) |
| profile `van` | Today `suv` + the partition above + ladder frame. | Drayman |
| `--engine mid` | Engine block behind the cabin bulkhead on rear rails + bulkhead, rear-of-bulkhead nodes `FrameRear`, aluminium rear subframe ×1.5 section, gearbox/half-shafts at the rear axle, side fuel tanks at the sills, **front rails × `REAR_RAIL_FACTOR`** (new: no engine block in the nose). | Lucciola |
| `--frame ladder` | Cab-floor rail nodes `FrameLeft/Right`, under-cab rail beams at rail section so the frame runs bumper to bumper. | Hauler, Bastion, Drayman, Furrow |
| `--accessory pushbar` (B's proposal) | 4 `FrontBumper` nodes 0.10 m ahead of the bumper, HSS tubes to the bumper-beam rail nodes, detach group 8, 15 000 N·s, 25 kg; × 2 bumper beam on non-`cruiser` bodies. | Cruiser, Wolfhound, Bastion |
| `--hood steel`, `--bumpers steel`, `--panels aluminium` | Material swaps (+ detach impulse/mass scaling for steel bumpers/hood). | many |
| `--gearbox transverse` | FF/transverse AWD: gearbox component on the engine's left end, not in the tunnel. | FF cars, Lindwurm |
| `--fuel-tank rear\|saddle\|sides` | 70s tank behind the rear axle, truck side-saddle tank, MR side tanks. | Ironside; Hauler, Furrow; Lucciola |
| `--rear-axle live` | Rear suspension mounts fail 0.04 m, wheels 0.07 m. | Ironside |

## Update after lead run 4

- Re-measured on A's torque converter and stateful on-demand coupling. Converter slip costs the
  automatics ≈ 0.2 s where traction is not the limit (Wolfhound 6.38 → 6.57 s, measured with and
  without), and helps where it is (Hauler 9.45 → 8.98 s, Cruiser 8.61 → 8.36 s, now in band).
  Wolfhound coupling now engages at 2 % front slip (was 5 %): 6.47 s, back in its band.
- B changed `DEFAULT_SECTIONS.cross_member` (0.15e-4 → 0.06e-4); `build_roster.py`'s `SEC` mirror
  updated so the era adjustment still finds the cabin cross-members (see the coupling note in Needs).
  Lattices regenerated on B's current `gen-lattice`; barrier sweep still 109/112 (03's two-box rear).

## Response to contract v0.2.0 (lead run 3)

| v0.2.0 item for 09 | Done |
|---|---|
| `fuel_capacity_l` (fuel in `mass_kg` at 50 %) | Set on all 14 cars (45-98 L, class-typical). `mass_kg` is now curb (full tank, as curb figures are quoted) − half a tank (0.745 kg/L) + driver, so every car is 17-37 kg lighter than before; springs, dampers, bars and inertias were re-derived from the new mass. |
| `torque_converter` on the 5 automatics | Ironside (stall 2200 rpm, 2.1×, couples 2800), Hauler (2200, 2.0×, 2600), Cruiser (2400, 2.0×, 2800), Wolfhound (2600, 1.9×, 3000), Bastion (2200, 2.0×, 2600). `wp_vehicle` ignores the field until A implements it; their 0-100 misses are marked "B" in VALIDATION.md until then. |
| `AwdOnDemand` for Lindwurm / Wolfhound | Lindwurm 95 % front / up to 50 % rear / engage at 5 % front slip; Wolfhound 90 / 50 / 5 %. Sim: Lindwurm 0-100 5.37 → 5.44 s, Wolfhound 6.35 → 6.40 s (lead's first-cut coupling; A tunes). |
| `BrakeDef` doc | Matches what the roster already assumed since run 2 (capacity per wheel, favoured axle full, other × (1−b)/b) — no data change. |
| `WheelDef::inertia_kgm2`, `DrivetrainDef::efficiency` | Left at the defaults (0.7·m·r², 0.85). |
| Tires | 02 added `all_terrain` and `vintage_bias`: Hauler and Ironside switched automatically. On them the Hauler skids 0.63 g and the Ironside 0.62 g, both below period/class figures (0.68-0.76 / 0.70-0.75) — see Needs. |

All 14 cars validate against the v0.2.0 schemas; `collect_results.py` re-run on the merged build
(VALIDATION.md); barrier sweep unchanged at 109/112 (the 03 two-box rear finding).

## Response to lead feedback (run 2)

| Item | Done |
|---|---|
| 1 Tires | Rebuilt: 10 cars now on their intended compound (`sport` ×7, `police_pursuit` ×3). Hauler (`all_terrain`), Ironside (`vintage_bias`) and the two `street` cars stay on `street`. Sim columns of `VALIDATION.md` filled from `tools/specsheet/validation_results.json`, written by the new `collect_results.py` (runs `wp_vehicle`'s metrics example + `damage-barrier --sweep`). |
| 2 Rampart / Wolfhound / Ironside | **Wolfhound 100-0: 61.7 → 39.9 m** (in band), Rampart 55.6 → 39.0 m, Cruiser 57.0 → 40.1 m — two causes, below. **Rampart 0-100: 6.3 → 5.9 s**, still over 4.5-5.0: it is not only traction (semi-slicks only reach 5.5 s; final drive, CG, LSD each ≤ 0.03 s) — `wp_vehicle`'s launch is conservative for powerful RWD (its debug coupe at 200 kW/t runs 5.2 s). Not fixed by inflating power; flagged to 02. Ironside 52.5 m: intended "scary brakes", in its band. |
| 3 Brake semantics | Re-derived for 02's model ("favoured axle full `max_torque_*`, other × (1−b)/b"). `max_torque_*` = caliper capacity per wheel; `bias_front` is now the proportioning valve: 0.5 when the rear caliper is the limit (both axles full), above 0.5 only to trim an oversized rear to the front's lock margin. Old data (b ≈ 0.7) starved the rear to ~40 %. Plus an **ABS ceiling**: delivered front ≤ 1.5 × front lock torque on the car's own tire (rear scaled with it) because 02's ABS releases to 45 % of the *commanded* torque — an oversized caliper keeps the wheel locked (measured on the Wolfhound: 5770 N·m → 47.3 m, 4500 N·m → 39.9 m). Spec sheets show raw capacity, the cap and delivered torques. |
| 4 `junkyard_beater` | Added: Beater, base price 3 000, late-80s FF 1.6 SOHC sedan, worn (tired engine, 0.18 damping ratio, slow synchro). Sim: 14.0 s 0-100, 0.83 g, 39.1 m, 163 km/h. |
| 5 / run 2 gen-lattice | All 14 lattices regenerated with `gen-lattice` (profile, real car mass, `--strength` from durability class 1-5 → 0.80/0.90/1.00/1.15/1.30, front overhang per car). The box-grid adjustments it covers are dropped (shaping, crumple rows, pillars, radiator/engine separation, doors, bed, node mass). What is still applied on top is listed below and in each spec sheet. Barrier sweep run on all 14 (VALIDATION.md): 109/112 checks pass; the 3 misses are one damage-model finding for 03 (two-box rear). |

## Exposes

- 17 cars, each `data/cars/<id>.json` (VehicleDef) + `data/cars/<id>.spec.md` + `data/lattices/<id>.json`:
  racers `vireo_xs`, `kestrel_ts`, `shiro_turbo`, `hakone_gt`, `rampart_50`, `ironside_440`,
  `sperber_32`, `lindwurm_4t`, `lucciola_350`, `hauler_1500`; police `sentinel_cruiser`
  (PoliceCruiser), `sentinel_wolfhound` (PoliceInterceptor), `sentinel_bastion` (PoliceSuv);
  economy floor `junkyard_beater` (Beater, 3 000); civilian traffic `tansy_13` (hatch),
  `drayman_cargo` (van), `furrow_24` (pickup). All load via `wp_contracts::load_car`.
- `data/cars/ROSTER.md` — roster table; `data/cars/VALIDATION.md` — sim + estimate vs class
  targets and the barrier matrix, with every miss explained.
- `tools/specsheet/` — `build_roster.py`, `collect_results.py` (sim metrics + barriers →
  `validation_results.json`), `specsheet.py`, `estimate.py`, `nameplate_check.py`,
  `smoke_sim.py` (simcli format v2), `pursuit_probe.py` (police acceleration/chase),
  `ram_sequence.py` (full-sim ram/PIT/T-bone sequence on a parked car), and the
  `ram_probe` crate (`ram-probe`: repeated rams; `vmax-trace`: slab-safe top speed). See
  `tools/specsheet/README.md`.
- Police push bar: 4 extra `FrontBumper` nodes ahead of the bumper, detachable **group 8**
  (moved from 3: gen-lattice uses 0-7 for bumpers, hood, trunk and doors), 15 000 N·s, 25 kg.
  Cruiser and Wolfhound also get gen-lattice's `cruiser` profile (reinforced rails, ×2 bumper
  beam); the Bastion (SUV) gets the ×2 bumper beam by hand.

## Needs

| From | What | Workaround now |
|---|---|---|
| 02 vehicle | **ABS should modulate relative to the wheel's lock torque** (or reduce pressure until slip recovers), not release to a fixed 45 % of commanded torque. Any car whose caliper exceeds ~1.6× its lock torque stops 7-15 m long. | ABS ceiling in the car data (`ABS_CEILING = 1.5` in `build_roster.py`); remove when fixed. |
| 02 vehicle | Launch/TCS for powerful RWD cars is conservative (Rampart 5.9 s; debug coupe 5.2 s at 200 kW/t vs ≈ 4.6 s class). Sim also stops ≈ 1-2 m longer than road tests. | Explained in VALIDATION.md (cause "A"); car data not inflated. |
| 02 vehicle | Re-tune `all_terrain` / `vintage_bias` together: on them the Hauler skids 0.63 g (class 0.68-0.76) and the Ironside 0.62 g (period 0.70-0.75). Chassis setup moves it < 1 % (measured), so it is the compound's peak or cornering stiffness. | Accepted as is; explained in VALIDATION.md. |
| 02 vehicle (A) | Implement `torque_converter` (contract v0.2.0); the 5 automatics carry data for it. | 0-100 misses on Hauler / Cruiser / Ironside marked "B". |
| 02 / lead | Lucciola skidpad 0.95 g on `sport` vs prompt-09 target ≥ 1.0 g (1.09 g on `semi_slick`, measured). Decide: stickier `sport`, semi-slicks on the Lucciola, or accept 0.95 (realistic for the archetype). | Left on `sport`. |
| 03 damage | **Two-box rear structure**: on the hatch and SUV profiles the passenger cell runs to the tailgate, so a 40 km/h rear barrier gave 0.22-0.37 m cabin intrusion and a total loss (Vireo, Lindwurm, Bastion, Tansy). | Since round 6b: luggage bay relabelled `FrameRear` in car data (`--cabin-end` proposal above); all 4 now pass. |
| 03 damage | `gen-lattice` options for the hand adjustments (table "Proposed gen-lattice options"), a `van` profile. | `adjust_lattice` in `build_roster.py`. |
| 03 damage | Police offset 56 km/h near miss: on the `cruiser` profile + push bar the struck-side suspension reaches 0.084 m vs its 0.09 m failure threshold (appeared with `ab6970d`). Fine by me (total loss anyway) — flagging in case the calibration expects a failure. | Accepted; explained in VALIDATION.md. |
| 06 police / lead | **Pursuit speed limit**: `pursuit_cap_mps` (day 40) with `target_speed_margin` 0.10 means no police car can catch a full-throttle target on a straight once the target passes 144 km/h. For "catch every street car at heat 3+", raise the cap at L3+ (≥ 50 m/s measured enough for the traffic sedan, Hauler, Ironside within 800 m) and/or margin ≥ 0.25. | Cruiser data tuned to reach the cap first (above). |
| 06 police / 03 damage / lead | **Arcade cop retirement**: in Arcade the damage model never totals a car under repeated rams (health levels off ≈ 0.2-0.3, total-loss health 0.1), so when a cop leaves the chase is decided by `wp_police`'s `disengage_health` 0.4. With the police package that is after 5 rams + 1 hard hit. If playtests still find cops fragile, lower the Arcade disengage health (e.g. 0.3) rather than toughening the car further. B's Arcade cosmetic/mechanical split (row 10) will raise every car's Arcade health. | Car data tuned against 0.4. |
| 03 damage | `gen-lattice --accessory pushbar` with mass / section / detach-impulse options, and per-component heavy-duty factors (`--hd radiator=1.5,…`), so the police package is generator output. | `police_package` step in `adjust_lattice`. |
| 06 police / lead | **Unit mix by heat**: `Sim::set_police_car` takes one model. At L3+ dispatch should mix in `sentinel_wolfhound` (catches sport cars) and `sentinel_bastion` (roadblocks), e.g. a per-level car list. | All units are cruisers. |
| 05 world / lead | **Traffic car pool**: `World::set_traffic_car` / simcli `TRAFFIC_CAR` use one car (`debug_sedan`). Use `tansy_13`, `drayman_cargo`, `furrow_24` (+ `junkyard_beater`, `debug_sedan`) as a weighted pool for moving and parked traffic, picked by the world's seeded RNG — e.g. 45 % hatch, 25 % sedan, 15 % pickup, 10 % van, 5 % beater. | Cars exist and validate; not wired. |
| 02 vehicle | `metrics::top_speed_kmh` drives off the rig's 10 km ground slab on slow cars (still gaining speed at ~95 s) and reports the free-fall speed (the van: 295 km/h). Stop at the slab edge or teleport back. | `vmax-trace` (`ram_probe` crate) used for Vmax in `collect_results.py`. |
| 02 vehicle | A light-truck tire compound (`lt_highway`): van and pickup stop 0.3-0.6 m shorter than their bands on car `street` tires. | Accepted; explained. |
| 03 damage | `gen-lattice` options for what is still hand-applied (list below), `uhss` material. | `adjust_lattice` in `build_roster.py`. |
| 03 damage | `SEC` in `build_roster.py` mirrors `lattice_gen::DEFAULT_SECTIONS` to recognise beam roles; if those sections change, the era adjustment silently stops matching. A role tag per beam (or exporting the sections) would remove the coupling. | Keep in sync by hand. |
| 08 economy | Mods: Kestrel AWD conversion, Hauler 4WD (should land its 0-100 in band), Ironside LSD + disc-brake conversion, tire upgrades. | Not created (data/mods is 08's). |
| 10 harness / lead | Run `nameplate_check.py` in CI. | Run by hand before each push. |

## Contract change requests

Decisions are in "Lead feedback" below; status updated here.

| # | Change | Status |
|---|---|---|
| 1 | `DrivetrainDef::torque_converter` | applied in v0.2.0; data set on the 5 automatics, waiting on A's implementation |
| 2 | On-demand AWD | applied in v0.2.0; Lindwurm and Wolfhound switched |
| 3 | Rear drum + fade | deferred |
| 4 | `BrakeDef` doc | applied in v0.2.0; roster data already matches |
| 5 | Solid axle | deferred |
| 6 | Roll centre height | deferred |
| 7 | `BodyRegion::Accessory` (push bar) | applied in v0.3.0; push bars on the three police cars use it since round 8 |
| 8 | `fuel_capacity_l` | applied in v0.2.0; set on all 17 cars |
| 9 | `VehicleClass::Van` (and/or a `Civilian` flag) — the cargo van and the compact pickup are traffic cars filed as `Truck`; economy/race class brackets and traffic selection should not treat them like the Hauler. | **new (round 6b)** |
| 10 | `VehicleClass::Race` + `VehicleClass::Offroad` — race cars are filed as `Exotic`, the rally-raid car and pre-runner as `Truck` (wave 3). | **new (wave 3)** |

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
8. **Passenger-cell end** (round 6b): luggage bay behind the rear seatback (hatch, SUV) and the
   van's cargo box behind the seat partition relabelled `FrameRear`.
9. **Mid-engine front rails** (round 6b): × `REAR_RAIL_FACTOR` (1.6), mirroring the
   generator's own reason for stronger rear rails.

## Changes outside my crates

- `data/audio/cars/junkyard_beater.json` (audio workstream's folder): `wp_audio`'s
  `shipped_data_is_valid_and_complete` test requires a sound definition for every car in
  `data/cars`. Added a plain NA inline-four definition (same packs as the other small fours,
  gain 0.9); the audio owner may retune it.
- `tools/specsheet/smoke_sim.py` (mine) has the lead's format-v2 update.
- Round 6b: `data/audio/cars/{tansy_13,drayman_cargo,furrow_24}.json` (new cars need an audio
  def, same reason as the Beater) and `data/audio/cars/sentinel_cruiser.json` switched from the
  V6 pack to `engine_proc_v8` / 8-cylinder cross-plane (the cruiser is a V8 now). Audio owner
  may retune.
- Round 8: `data/panels/{sentinel_cruiser,sentinel_wolfhound,sentinel_bastion}.json` and
  `data/panels/meshes/` for the same cars (R's folder): regenerated with R's own
  `gen-panels` / `gen-car-mesh` because the push-bar nodes left `FrontBumper`
  (`tools/assets` test `every_car_has_a_valid_up_to_date_mesh_and_panel_spec` failed on
  "node 300 is not a free node of FrontBumper"). Checked first that main's specs for those
  three cars were exactly generator output (`gen-panels --check` clean against main's
  lattices), so no hand tuning was lost; `--check` is clean for all cars after.
- Round 8: `data/tires/*.json` + `data/tires/README.md` (02's folder; assigned to H by the lead).
- Round 6b: root `Cargo.toml` — one workspace member line, `tools/specsheet/ram_probe` (a tool
  crate like `tools/tuning-dash/probe`; not reachable from `wp_wasm`).

## Perf numbers

| Measurement | Machine | Native | Browser | Budget |
|---|---|---|---|---|
| Lattice size per roster car | — | 244-304 nodes, 1539-1988 beams | same | prompt 03: 0.25 ms/car lattice step (03's `damage-bench` covers it) |
| `build_roster.py` (17 cars incl. gen-lattice runs) | cloud container | 1.0 s | — | — |
| `collect_results.py` (metrics + Vmax + 17 barrier sweeps) | cloud container, release | ≈ 20 s | — | — |
| `pursuit_probe.py --chase` (19 cars × 40 s through simcli) | cloud container, release | ≈ 1 s | — | — |
| `ram-probe sentinel_cruiser` (2 modes × 4 kinds × 3 speeds × 6 rams) | cloud container, release | ≈ 2 s | — | — |

## Known gaps

- Pursuit: the cruiser cannot catch a full-throttle Kestrel or any sport car on a straight;
  with today's police speed cap it cannot catch anything above 150 km/h top speed (request above).
- The ram sequence hits a *parked* police car (strikers do the moving); a cop ramming at speed
  while driving is the same closing speed but not bit-identical. Wolfhound and Bastion did not
  get the police package (only the cruiser is spawned by the sim); same inputs apply if wanted.
- Sim misses (VALIDATION.md, all explained): small "sim conservative" overruns (Kestrel,
  Hakone, Sperber 0-100 by 0.02-0.3 s; Hakone/Sperber/Lindwurm/Rampart 100-0 by 0.5-1 m),
  Rampart 0-100 and skidpad, Lucciola skidpad (decision above), automatics' 0-100 (torque
  converter pending), three tire-limited skidpads 0.01-0.02 g under.
- The ABS ceiling and the `SEC` mirror are workarounds (see Needs).
- Automatics' converter is data-only until A implements it; live axles and drum fade are
  approximated (deferred requests 3, 5).
- Prices and durability classes are first-pass; 08 should tune prices against the economy
  (the lead set the three civilian cars' prices in round 8; mirrored in `roster_inputs.py`).
- Police barrier/ram crush figures include the push-bar tubes until B excludes `Accessory`
  from the crush metrics (round 8 request); health and verdicts are unaffected.

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


## Lead feedback (round 6b, 2026-10-19) — head-start tasks

Contract **v0.3.0** is being applied in parallel by the lead (branch `lead/contract-v0.3.0`). Do not edit `wp_contracts`; build your work so it drops onto v0.3.0 (inherent methods / local types now, trait wiring after). Merge `origin/claude/laughing-newton-0xqr6q` (the lead branch, ahead of `main` until today's gate finishes) before starting. Goal of the project right now: the **single-player greybox demo**: drive in the city, raise heat, get chased by physical police, escape or get caught, and pay the repair bill.

1. Police cruiser (`sentinel_cruiser`) pursuit tuning: it must catch every street car in the city at heat 3+ on straights within reason, and survive about 3 rams in Arcade. It has a push-bar accessory (front lattice stiffer).
2. Traffic variety: add 3 cheap civilian analog cars (hatch, van, pickup) with lattices, for traffic and parked cars. Analog names only, per `data/cars/README.md`. Validate them as usual.
3. `gen-lattice` regeneration pass; fold the hand adjustments back into the generator where possible.


## Lead feedback (round 8, 2026-10-19)

Contract **v0.3.0 is on `main`** (typed police/race/economy events, damage snapshot/summary, road-network trait, weather `LocalConditions`; see `docs/CONTRACT_CHANGELOG.md`). Workspace tests now take about 3.5 minutes (`lead/test-speed`). Owner playtest items are tracked in `docs/PLAYTEST_LOG.md`.

1. `semi_slick.json`: `treaded: false` (W-4).
2. Tire data for `PaintedLine`.

## Lead feedback (round 12b, 2026-10-19) — junkyard beater (backlog B06)
Base: `origin/lead/round12` (round 12's merges; `main` catches up when its gate is green), not main.
1. Add `data/cars/junkyard_beater.json`: the economy's floor car (G's career already hands it out
   — see `wp_economy` / `wp_wasm::career` and `docs/integration/economy.md`). A tired 1980s-style
   compact FWD sedan analogue (no real names): ~1,050 kg, ~65 kW, 4-speed manual/3-speed auto,
   open diff, soft worn suspension, old tyres (lower grip), low durability, cheap parts. It
   should be slow but fun and *readable* (body roll, squeal). Lore line in `CARS_LORE.md`.
2. Its lattice + panel mesh: reuse the closest existing body (coordinate with R's panel pipeline
   via `docs/integration/panels.md` if a new mesh is needed; a placeholder generated mesh is fine).
3. Validation band in `data/cars/VALIDATION.md` (0–100 km/h ~14–16 s, top speed ~150 km/h),
   tests, price consistent with the economy floor. Add it to the testbed car list.

## Request from B (damage, round 12b, 2026-10-20) — police toughness in Arcade (playtest row 48)
B retuned Arcade (`data/modes/arcade.toml` + Arcade-only `wp_damage` knobs; table and method in
`docs/integration/damage.md` "Response to round 12b"). In the full sim the cruiser with today's
police package now stays in the chase through 8 rams / PITs / side-swipes / T-bones at 30–90
km/h (before: one 50 km/h T-bone or wall hit pulled it over) and outlasts `hakone_gt` on 90 km/h
wall hits (wrecks on hit 5 vs 4). **No police package change is needed for the playtest item.**
- **Optional (H's call)**: repeated 70 km/h *rigid-wall* hits end the cruiser on hit 6 (cabin
  intrusion > Arcade's 0.6 m after ≈ 2.2 m of crush through the long nose and push bar) while
  the sports car limps on past 8. A slightly stiffer firewall / cabin front (or less crushable
  length behind the push bar) would make the cruiser tougher in that case too. Measure with
  `cargo run --release -p simcli --example damage_toughness -- arcade sentinel_cruiser hakone_gt`
  (`WP_KINDS=frontal` for just the wall rows).
- Sim tables are unchanged by B's work (without panels; with panels a hanging fender no longer
  counts as a failed suspension in either mode), so `VALIDATION.md` sim columns stay valid.
## Round 12b (A, 2026-10-20) — ride data changes for "too bouncy / launched too easily"
Made by A in coordination (lead round 12b feedback to 02, playtest row 54); H owns the files
and is welcome to retune within the targets. Measured with
`cargo run --release -p wp_vehicle --example ride all` (see `docs/integration/vehicle.md` →
"Round 12b response").
1. **Dampers, all 19 cars:** `damper_bump_ns_per_m` × 1.2, `damper_rebound_ns_per_m` × 1.6
   (rounded to 10 N·s/m). Damping ratio was 0.17–0.31 bump / 0.26–0.47 rebound; now
   0.21–0.38 / 0.42–0.75 (rebound ≈ 2× bump, real-car practice). Each car keeps its
   character relative to the others (the beater and ironside_440 stay the softest). The data
   values are now the dampers' **low-speed** rates: the model blows off above 0.5 m/s.
2. **Preload** where a car sat on its bump stops at rest (static bump travel < 35 % of the
   travel → 45 %): drayman_cargo F 0.03 → 0.059; furrow_24 F 0.02 → 0.06; ironside_440 F 0.03
   → 0.11, R 0.03 → 0.076; junkyard_beater F 0.01 → 0.071, R 0.01 → 0.039; tansy_13 F 0.01 →
   0.061, R 0.01 → 0.031. Spring rates and ride height are unchanged (the model places the
   static point by the hub position); only where the car rests within its travel moves. Bump
   travel left at rest was 6 mm (beater front), 10 mm (ironside front), 17 mm (tansy front).
3. No other fields changed. Ride frequencies were already car-like (1.06–2.08 Hz) and the
   inertia tensors are fine (75–97 % of a solid box of the body size).
Please mirror these in the `*.spec.md` sheets and `VALIDATION.md` (handling numbers moved
slightly: understeer gradient −0.37 to +0.04 deg/g, held-key g −0.01 to +0.03; table in
`vehicle.md`). Rule of thumb for new cars: rebound ≈ 2× bump, bump ≈ 0.35 of critical, ≥ 45 %
of the travel left to the bump stop at rest; `tests/ride.rs` checks all three for the roster.
## Rows 56 / 72 (A, 2026-10-20) — handbrake torque
`brakes.handbrake_rear_nm` raised on 18 of 19 cars to ≥ 1.5 × the static rear lock torque
(rear corner load × rolling radius), so a pulled handbrake locks the rears on every car (owner
row 72: "I do see the rear wheels lock" — on most cars they did not: 14–25 rad/s left after a
0.6 s pull at 60 km/h). Before → after, N·m per wheel: debug_coupe 1500 → 1750, debug_sedan
1200 → 1310, drayman_cargo 1540 → 2980, furrow_24 870 → 1340, hauler_1500 1620 → 2970,
ironside_440 600 → 1890, junkyard_beater 600 → 830, kestrel_ts 830 → 1280, lindwurm_4t 1100 →
1410, lucciola_350 1330 → 1860, rampart_50 1430 → 2020, sentinel_bastion 1620 → 3280,
sentinel_cruiser 1440 → 2390, sentinel_wolfhound 1440 → 2330, shiro_turbo 1010 → 1330,
sperber_32 1230 → 1850, tansy_13 600 → 840, vireo_xs 600 → 900 (hakone_gt unchanged, 1780).
Rule of thumb for new cars: `handbrake_rear_nm ≥ 1.5 × m × (1 − front_weight_frac) / 2 × g ×
rear radius`. Please mirror in the spec sheets.
## Request from B (damage, round 13, 2026-10-20) — cruiser driveshaft (playtest row 87)
B softened Arcade damage consequences ~20–30 % (`docs/integration/damage.md` "Round 13"). The
cruiser now outlasts `hakone_gt` in every measured row except three 100 km/h hits in a row
(wall, offset wall, angled wall), where it ends on hit 3 and the sports car on hit 7: its
**driveshaft** fails (`intrusion_fail_m` 0.10, the civilian value; the police package already
has radiator / rack thresholds ×1.5). **Request (optional, H's call):** `sentinel_cruiser`
driveshaft `intrusion_fail_m` 0.10 → ~0.14 (and `intrusion_total_m` to match). The 0.6 m cabin
case from round 12b is covered: Arcade's limit is now 0.78 m. Measure with
`WP_KINDS=mixed,wall-angle WP_SPEEDS=100 cargo run --release -p simcli --example damage_toughness -- arcade sentinel_cruiser hakone_gt`.
