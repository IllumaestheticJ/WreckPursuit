# Car roster integration notes

- **Workstream / prompt:** 09 — car roster data
- **Branch:** `ws/cars`
- **Crates / folders owned:** `data/cars/` (roster files), `data/lattices/` (roster car files, not the generator), `tools/specsheet/`; round 8: `data/tires/` edits the lead assigned to H (`PaintedLine`, `treaded`)
- **Contract version built against:** 0.3.0

## Street-spec power rebalance (2026-10-20)

Owner feedback: compared with the sim-cade feel reference, our cars were heavier and had less
torque, and ran 2-6 s slower 0-160 km/h (≈ 0-100 mph). The Sport and Muscle cars now carry a
**street build at ≈ 240 W/kg** (`VehicleDef.mass_kg`, with driver and half a tank). Every car
keeps its stock-class torque-curve *shape*: one scale factor per car, stated in its spec sheet's
engine `why`. Curb mass is trimmed ≈ 5 %. Source of truth: `roster_inputs.py`
(`STREET_SPEC_W_PER_KG`). Beaters, compacts, trucks and police are unchanged.

| car | mass kg | kW | W/kg | 0-100 s | 0-160 s (`pursuit_probe.py`) | Vmax km/h |
|---|---|---|---|---|---|---|
| kestrel_ts | 1453 → 1383 | 181 → 318 | 125 → 230 | 7.08 → 5.98 | **15.85 → 10.58** | 248 → 260 |
| shiro_turbo | 1301 → 1241 | 182 → 297 | 140 → 240 | 6.43 → 5.28 | **14.32 → 9.65** | 260 → 311 |
| hakone_gt | 1599 → 1519 | 242 → 372 | 151 → 245 | 5.87 → 5.25 | **12.55 → 9.18** | 282 → 317 |
| sperber_32 | 1602 → 1522 | 248 → 373 | 155 → 245 | 6.07 → 5.12 | **12.90 → 9.33** | 272 → 316 |
| lindwurm_4t | 1555 → 1484 | 220 → 327 | 141 → 220 | 5.43 → 3.95 | **12.53 → 8.30** | 260 → 291 |
| rampart_50 | 1753 → 1663 | 319 → 416 | 182 → 250 | 5.85 → 5.57 | **11.50 → 9.67** | 268 → 303 |
| ironside_440 | 1695 → 1615 | 216 → 380 | 127 → 235 | 8.15 → 8.05 | **17.22 → 12.90** | 210 → 248 |
| lucciola_350 (Exotic) | 1344 → 1294 | 278 → 356 | 207 → 275 | 4.57 → 4.35 | **9.58 → 8.12** | 290 → 319 |

- **Ironside:** uniform scaling gave 985 N·m, which isn't credible. It is reshaped instead as a
  street-built big-block: peak 700 N·m at 4000 rpm, 380 kW at 5900, redline 5200 → 6200,
  limiter 5500 → 6500, idle 650 → 700. Its 0-100 barely moves because the bias-ply tires limit
  the launch (still the known 8.1 s miss). The build shows from 100 km/h up.
- **Lindwurm** sits at 220, not 240, on purpose. At 235 its AWD launch beat the exotic to 160 km/h.
- **Lucciola** went above the pack (275 W/kg) so the exotic stays on top. Its final drive went
  4.30 → 3.90 because it was rev-limited at 291 km/h, below the sports cars.
- **0-100 bands** (`specsheet.py` TARGETS) moved to the street-spec figures. These are
  traction-limited launches, so 0-160 is the figure the rebalance was tuned on. Skidpad and
  100-0 bands are unchanged. Hakone and Sperber 100-0 are now in band (lighter). Rampart 100-0
  is 0.2 m over (was 1 m).
- **Unchanged:** barrier sweep 135/136 (the same Wolfhound offset near miss). Every car
  smoke-sims deterministically. Nameplate check is clean. `pursuit_probe.py` gained a 0-160 column.
- **Police (for E / lead):** catch outcomes are unchanged (the cruiser already could not catch
  any sport/muscle car on a straight). The gap is wider now: cruiser 14.0 s and Wolfhound
  13.2 s to 160 km/h, against 8.1-12.9 s for these cars. If pursuits get too easy to escape on
  straights, the cop cars need the same treatment. Not done here (out of scope).
- **Kestrel price 22 000 → 27 500.** `roster_prices::pricier_cars_are_not_slower` failed:
  the big-turbo Kestrel rated PI 659 against the 34 000 Lindwurm's 638. At 27 500 the Kestrel
  is above 34 000 / 1.25 = 27 200, so the Lindwurm no longer counts as 25 % pricier than a
  higher-rated car. That is a small raise, and it fits a 318 kW build. The test's
  `KNOWN` outliers (Hakone, Ironside) are unchanged.
- **For G (economy):** the class index went up for these 8 cars (now Sport 638-699, Muscle
  570-718, Exotic 769). `wp_economy::estimate` rates the Lindwurm lowest of the sport cars
  (PI 638), but in the sim it is the quickest of them to 100 km/h (3.95 s). The point-mass
  launch seems to underrate AWD; worth a look. Your README tuning table and
  `docs/design/{CARS_LORE,GDD}.md` still quote the Kestrel at 22 000 (not edited: not my files).
- **Changes outside my crates (test-only, to keep the workspace green):**
  - `wp_economy/tests/roster_prices.rs`: `hakone_gt` dropped from `KNOWN`, because it is no
    longer a value-for-money outlier (economy.md says to remove fixed outliers ourselves).
  - `wp_vehicle/tests/gearbox.rs`: the Hakone launch window went 8.0 → 6.5 s. The faster car
    now shifts 1→2 / 2→3 / 3→4 at 2.9 / 5.2 / 7.8 s, so 8 s caught a third shift.
  - `wp_vehicle/tests/slide_control.rs` (`wheel_profile_is_bit_identical_to_the_plain_model`):
    its hashes were recorded on the old Rampart/Lucciola data. The test now loads those
    definitions from `tests/fixtures/pre_street_spec/` (copied from `60c236a`), and all four
    hashes match. So the code invariant is still tested, and no hash was re-recorded.
  - `wp_vehicle/tests/slide_control.rs` (`keyboard_power_oversteer_exit_is_recoverable`,
    after merging main's #27 grip-capped keyboard lock): circle 0.7 → 0.65 g. At 0.7 g the
    street-spec Rampart reached the throttle point in third with TCS/ESC off, which breaks the
    test's "scripted in second gear" precondition. At 0.65 g every case is in second, and
    every case still recovers.
- **Checks:** `cargo fmt --check`, `cargo clippy --workspace -- -D warnings`, wasm build of
  `wp_wasm`, `cargo test --workspace --release --no-fail-fast`. The only failures were the 3
  above, all now passing. No simcli golden changed.

## Police speed caps raised (2026-10-20, owner request)

`data/police/pursuit.toml [pursuit_cap_mps]` ×1.5: day 40 → 60, dawn/dusk 36 → 54, night
32 → 48 m/s. The owner asked for it; it is recorded in `police.md` ("Changed by other sessions")
because the file is E's. `pursuit_probe.py` now reads the caps and margin from that file
instead of a hard-coded copy. Chase model (`--chase`, day cap, +10 % margin), what changed:
the cruiser now also catches the Ironside, and the Wolfhound now catches the Kestrel,
Ironside, Bastion and the debug coupe (the Rampart and Cruiser only with the margin rule
ignored). The sport cars and the exotic still escape a straight drag race from a 30 m head
start: with equal power that is a matter of tactics, not the cap. Tests and goldens unchanged.
`docs/design/DEMO_GREYBOX.md`'s player-facing cap line is updated (173 / 216 km/h).

Follow-up (2026-10-21), also on owner request: a new `[unit] physical_accel_mps2 = 5.0`, the
launch ceiling for AI cops driving real cars (was the shared 4.0 `accel_mps2`, which stays for the
headless kinematic driver). Details, the 6.0 attempt and the scenario changes are in `police.md`.

## Street-spec rebalance, police cars (2026-10-20, follow-up)

The owner asked for the same treatment on the police cars. Same method: curb mass trimmed ≈ 5 %
(2000 → 1900 kg, Bastion 2600 → 2470) and one torque scale factor per car. Each engine is now a
supercharged / bigger-turbo build. Order kept: interceptor fastest, SUV slowest.

| car | mass kg | kW | W/kg | 0-100 s | 0-160 s | Vmax km/h |
|---|---|---|---|---|---|---|
| sentinel_wolfhound | 2049 → 1949 | 272 → 468 | 133 → 240 | 5.55 → 3.98 | **13.15 → 7.85** | 244 → 311 |
| sentinel_cruiser | 2048 → 1948 | 276 → 458 | 135 → 235 | 6.38 → 5.62 | **14.03 → 9.87** | 260 → 296 |
| sentinel_bastion | 2638 → 2508 | 265 → 502 | 100 → 200 | 7.97 → 5.50 | **20.15 → 10.68** | 214 → 270 |

- **Bastion at 200, not 240:** on 2.5 t, 240 W/kg would need > 1000 N·m from a truck V8. It is
  the heavy rammer, not a chaser. Raise it in `roster_inputs.py` if wanted.
- **Validation:** the police 0-100 bands moved to the street-spec figures. All three cars are now
  in every band (the old Wolfhound and Bastion skidpad near-misses are gone). Barrier sweep
  unchanged: 135/136, with the same Wolfhound offset near miss. Smoke sim deterministic.
  Nameplate check clean.
- **Durability (`ram-probe`, 3 cars × 2 modes × 4 ram kinds × 3 speeds):** health after each ram
  moves ≤ 0.03. The disengage ram or total-loss ram moved by one ram in 6 of 72 cases, in both
  directions. Effectively unchanged.
- **For E (police) / lead — the speed cap is now what limits pursuits.** Straight-line catch
  results (`pursuit_probe.py --chase`) did not change: the cruiser and the Wolfhound still catch
  the traffic/cheap cars and no sport/muscle car. The cop cars now accelerate like the racers,
  so the limit is `wp_police`'s pursuit speed `min(target × 1.10, cap)` with the 40 m/s
  (144 km/h) day cap. Every street-spec car passes 144 km/h in 6.9-11.5 s and keeps going. If
  pursuits should catch racers on open roads, the caps in `data/police/pursuit.toml` need
  raising (not my file; not changed).
- **Slide assist (for A):** `wp_vehicle::slide::slide_class` scales slide help with
  power-to-weight from 120 to 250 W/kg. So every street-spec RWD car, the Bastion included
  (0.71), now gets near-full help on keyboard/gamepad profiles. Its unit test
  `class_favours_powerful_rwd` used the Bastion as its "slow RWD → floor" example; it now uses
  `hauler_1500` (109 W/kg, test-only change).
- **Goldens re-blessed** in a separate `[bless]` commit: 11 city/race/traffic scenarios that spawn
  `sentinel_cruiser` as the city police car (golden_city_chase, golden_night_pursuit_escape,
  opponent_{circuit_8x5, flee_police, open_roads_traffic, reverse_grid_2laps},
  race_city_sprint{,_police}, world_traffic_soak{,_observer,_vesper}). Every scenario's own
  range expectations still pass; only the state hashes moved.

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
