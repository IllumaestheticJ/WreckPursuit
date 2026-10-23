# Ashpeak County port: PR #50 onto `lead/round12` (`lead/ashpeak-port`)

- **Who:** lead helper (Ashpeak port), 2026-10-23.
- **Source:** PR #50 "Ashpeak County: new map region around Port Vesper, county sheriff skin,
  honk-to-start race spots", `ws/ashpeak` @ `9565482e` (35 commits on main `06d2b260`).
- **Base:** `origin/lead/round12` @ `db0035d9`. **Pushed to:** `lead/ashpeak-port` only.
- **Contract:** 0.3.1 (round 12's). No contract edit; requests below.
- **Not touched:** `crates/wp_vehicle` (physics freeze), `crates/wp_contracts`, `schemas/`.

## Method

`ws/ashpeak` sits on main, and main has PRs #41 (free-roam racers, `wp_opponent::roam`) and
#45 (career time progression) that round 12 never took. A merge of `ws/ashpeak` would also
claim those two. So the PR's own diff (`06d2b260..9565482e`, 96 files) was applied as one
3-way cherry-pick onto round 12 (base = the PR's main parent): exactly PR #50, nothing of
#41 / #45. 22 conflicts, as expected. Commits on `lead/ashpeak-port`:

| Commit | What |
|---|---|
| `9c1d26b8` | The port: conflict resolutions, honk spots on round 12's challenge system, police profiles, name fixes |
| `6408ed48` | Greyboxes and the grid road file regenerated on the tip; `pick_harlan_dirt_run.ron`, `pick_waterfront_drag.ron` rewritten (`WP_RACE_WRITE_SCENARIOS=1`) |
| `f23212ed` | testbed: duplicate `debugPlace` binding dropped (tsc) |
| `b5281e40` | `[bless]` the 8 Ashpeak race goldens |
| `10f7cdfa` | Merge round 12's new tip `5080fa80` (main #41 / #45 / #48, touge port #49, overpass fix, police row 156): 9 conflicts, below |
| `a04af158` | Ashpeak greybox regenerated on the merged tip (round 12's overpass fix); Vesper and grid identical to `5080fa80` |
| (this doc) | docs, perf, gate |

### Merge of round 12 `5080fa80` (`10f7cdfa`)

Round 12 moved while the port ran (main merged in: #41 free roam, #45 career time, #48 damage
modes / contract 0.4.x; the touge port of PR #49; overpass walls row 159; police row 156).
Conflicts, both sides kept: `wp_race` `RaceCity` has `Ashpeak` and `Touge(id)` (`hosts()` of a
pass is a one-element slice of a static copy of `TOUGE_IDS`; the touge `RaceDef` gets
`grid: None`, it sets its own single-file grid); `wp_wasm` session: meet spots + the one horn
edge next to main's free roam (renumbered step 1f; roamers do not read the horn); simcli city
presets, `gen-city` help, scenario docs, `wp_race` README, wasm lib docs, testbed `main.ts` /
`sim.ts`. Round 12's `027525c8` (street challenge only from right behind the racer) keeps the
honk priority as designed; the priority test now parks the player right behind the racer.

## Round 12 kept (all of it)

Port Vesper's round-12 work is untouched by the port; the `vesper_v2` greybox regenerates
**byte-identical** to round 12's and every round-12 golden (Vesper ones included) matches:

- Loop 9 rest stops (`World::rest_stops`, off / on ramps, `PoiKind::RestStop`, no houses along
  their parking roads), the Hollins Tunnel exit point, bridge piers kept off roads;
- Priority-junction starvation release, traffic cap / LOD demotion, light / dormant vehicle
  modes (files the PR does not touch);
- street racers (8 by day; 13 at night, 4 on Loop 9; horn challenge; night meets at the rest
  stops): `wp_wasm::street` unchanged except the horn edge (below). `e2e/street.mjs` passes.

## Conflict resolutions

| File | Resolution |
|---|---|
| `wp_world/src/vesper.rs` `covered_spans` | Round 12's Loop 9 `loop_cover` (one span on the ring centreline) for Loop 9 drafts; single-road plans as round 12; Ashpeak's multi-carriageway midline tube only for its open freeways' divided drafts (`divided(draft)`). Signature `(g, built, ring, loop_drafts, remap, divided)`. |
| `vesper.rs` `buildings` | Takes both round 12's `no_buildings` (rest-stop roads) and Ashpeak's `reserved` / `reserved_decor` / `allowed`; uses Ashpeak's shared `SurfaceGrid`. Call site keeps `&rest_roads`, adds Ashpeak's region buildings. |
| `vesper.rs` (clean merge, fixed to compile) | `REST_ROAD` style gains `material: RoadMaterial::Asphalt` (road format v6). |
| `wp_world/src/map.rs` | Round 12's `Arc` paint and deck grid + Ashpeak's per-road material table. |
| `wp_world/src/network.rs` | `ROAD_FORMAT_VERSION` 6 with round 12's v5 note kept. |
| `wp_world/src/props.rs` (round 12 only) | `clear_of_decor` covers the new decor kinds (pine trunk; crane / barrier / site fence / hay bale / billboard boxes, as `decor_collider`). |
| `wp_race/src/catalog.rs` | Ashpeak's `def.grid` override, then round 12's `fit_grid` (closes up / single-files on narrow roads). Crowfoot ascent gets `finish_back_m: 0.0` (round 12 field; the saddle is a through road). Both tests kept. |
| `data/police/races.toml` | Round 12 requires one profile per picker race (`every_race_has_its_own_police_profile`): 8 new tables, see below. |
| `wp_wasm/src/lib.rs` | `race_start_tuned` (round 12's tune) + Ashpeak's "stay on the current map" (`race_city`, `race_scenario_in`). Ashpeak's `debug_place` dropped: round 12 has the identical function. |
| `wp_wasm/src/lod.rs`, `police.rs` | Round 12's model column + Ashpeak's county flag (4). |
| `wp_wasm/src/session.rs` | See "Honk-to-start spots". `crate::roam` (main #41) not brought in. |
| `tools/city-import/src/glb.rs` | Round 12's freeway-taper material first, then highway, then Ashpeak's gravel / dirt. |
| `tools/city-import/tests/junction_cover.rs` | Both lists (`ramp_taper` + `gravel_road` / `dirt_road`). |
| `apps/testbed-web/src/render/police_units.ts` | Stays deleted (round 12 retired it: police units are fleet / LOD-drawn, light rig in `render/lights.ts`). Ashpeak's county livery moved to `fleet.ts` (county `FleetModel`, `styledPaint`), `lod.ts` (county model cache, green fallback paint) and `vehicle_view.ts`. |
| `render/fleet.ts`, `lod.ts`, `vehicle_view.ts` | Round 12's versions + the county variant (above). |
| `render/livery.ts` | Round 12's `liveryFlag()` / `setLivery(tex, base)` with Ashpeak's `county` argument. |
| `wasm/sim.ts`, `main.ts`, `package.json` | Both sides (tuned race start + spots; street marker + spots view; `e2e:tune` + `e2e:race-spots`). Duplicate `debugPlace` removed. |
| `docs/integration/race.md` | Both sections; Ashpeak's notes on the honk check annotated with the port. |
| `data/city/procedural_12x12.road.*` | Not hand-merged: regenerated (format v6). |

## Honk-to-start spots on round 12's one challenge system

Owner ask: Ashpeak's spots use round 12's single challenge system (the horn action) and its
existing betting, not a parallel one.

- **One horn edge.** `Session` owns `horn_was`; each live tick computes one rising edge
  (`honk`) from the player's `VehicleInput::horn` and hands it to `StreetRacers::before_step`
  (which no longer tracks the horn itself). If a roaming street racer is within
  `CHALLENGE_RADIUS_M` it takes the honk (`StreetStep::challenged`); otherwise
  `Session::observe_horn` checks the spot the player stands in (< 3 m/s). Ashpeak's own
  `HonkTrigger` is gone (`wp_race::spots::honk_spot` is the pure "which spot" helper).
- **Same rules as a street challenge:** no spot while the player is wanted or chased (row
  150), with a street offer or street race open (`StreetRacers::busy`), or with the run over;
  never in a race, replay, scripted or career session.
- **Money.** A spot starts the picker race (`race_start`), whose existing stake applies: the
  advertised buy-in and purse (`race_list`, `data/events/stakes.toml`). The spot prompt now
  shows it ("Honk (Q) to start: Hollins Drive hill sprint — Sprint, 941 m — buy-in $553,
  purse $5,031"). No second betting system. (PR #47's countdown bets would apply to spot races
  as to any picker race.)
- Still read-only: the check runs before the step and writes nothing to the sim (hashes and
  replays unchanged).
- Tests: `race_spots_tests::street_racer_in_reach_takes_the_honk_first` (new), the PR's spot
  tests (street racers off so none is in reach), `spots::tests::honk_starts_the_spot_only_when_stopped_inside`.

## Design choices (other)

- **Police profiles for the 8 Ashpeak races** (`data/police/races.toml`), first proposals for
  E: presence = the stake table's default for the legality (1.0 street, 2.0 illegal daytime,
  as on `ws/ashpeak`). Styles: Mountain tail (Crowfoot up: no rams / PITs, stakeout at the
  saddle), Valley net (Crowfoot down: junction block at the foot), Open country (Harlan:
  helicopter, no spikes on gravel, backup ×0.75), Downtown crackdown (illegal daytime: all
  tactics, stakeout near the end), Tunnel exit (Route 7), Pass patrol (Grizzly Grade), Late
  response (drag: backup ×0.5, no tools), Block grid (metro circuit). All 8 still race to the
  flag (`race_picker`, release).
- **Names (owner rule).** Renamed real names: "Harlan County Sheriff" (a real Kentucky
  agency; also contradicted the ACSO lore) → "Ashpeak Sheriff Harlan Substation";
  "Cannery Row" (Monterey) → "Cannery Lane"; "Grand Concourse" (the Bronx) → "Grand
  Promenade". The sheriff livery prints only "SHERIFF" / "ASHPEAK COUNTY" with a drawn star
  (no seal); police cars are round 12's analog `sentinel_*` models. Kept but flagged: "Harlan"
  (a common surname, also real US county names), "Crowfoot" (a Banff peak / glacier name),
  "Fifth Avenue" (generic), the internal scheme id `acso` (real agencies use that acronym; not
  shown in game).
- **wp_opponent change from the PR kept** (surface-aware speed profile): AI only, exactly 1.0
  on paved surfaces, so no Vesper golden moved. Not a `wp_vehicle` change.
- **Not ported:** main's #41 `wp_opponent::roam` / `wp_wasm::roam` (round 12 has its own
  street racers) and #45 progression. Ashpeak's spots never depended on them.

## Generated files

Regenerated on the tip with the justfile's commands (`just` is not installed in this container,
so `gen-city --grid 12x12 --seed 1`, `--preset vesper_v2 --seed 1`, `--preset ashpeak --seed 1`
directly):

| File | round 12 | port tip | Cap |
|---|---|---|---|
| `vesper_v2.greybox.glb` / `.b64.txt` | 10,039,688 / 13,386,252 | **identical** | 10.2 MB test; 15 MB preview |
| `procedural_12x12.greybox.glb` | 3,013,592 | identical | |
| `procedural_12x12.road.bin` / `.json` | v5 | v6 re-encode (new fields, same city) | |
| `ashpeak.greybox.glb` / `.b64.txt` | — | **25,026,264 / 33,368,352** | **over the 15 MB preview per-file cap** |

**The Ashpeak greybox is over the cap** (it already was on `ws/ashpeak`: 24.9 MB; its own
staleness test allows 30 MB). Not raised or hidden here; `tools/preview_build.sh` copies every
`*.greybox.b64.txt`, so a preview publish would fail on it (its `find -size +15M` prints it).
Measured on the tip (per-triangle split of the greybox primitives at Port Vesper's extent,
`wp_world::ashpeak::plan::VESPER_LO..HI`): **~71 % of the bytes are county geometry** (≈ 17.8
MB of the 25.0 MB `.glb`, ≈ 23.7 MB as `.b64.txt`); Port Vesper's part is the `vesper_v2`
greybox again. In the county the biggest item is the drawn ground (`ground_suburb`, ≈ 31 %),
then sidewalks / lots / kerbs / retaining walls (the metro grid), rock (mountains) and
buildings. Options for the lead:

1. **Split into tiles** (recommended): `ashpeak` loads the `vesper_v2` greybox plus county
   tiles. The county alone (~17.8 MB glb / ~23.7 MB b64) still needs **≥ 2 tiles** (e.g. west:
   farms + mountains, east: metro + freeways) to stay ≤ 15 MB each as b64. Port Vesper is then
   shipped once, not twice. Needs a small loader change in `scene.ts` (several files per city)
   and `gen-city` writing per-tile files.
2. **Coarser drawn ground** on the county (16 m → 32 m cells): ≈ −4 MB glb. Not enough alone
   (county would still be ~13–14 MB glb, ~18 MB b64); useful with 1.
3. **Leave Ashpeak out of the preview** (`preview_build.sh` skips files > 15 MB with a note;
   the map stays in dev / native).
4. Raise the cap: not possible for the artifact host (15 MB per file is the host's limit).

## Goldens

- Every round-12 golden matched unchanged (incl. `jam_pursuit_vesper`,
  `city_tunnel_hollins_drive_through`, `race_loop9_circuit`, opponent pace on Vesper).
- **Blessed (`b5281e40`):** the 8 new Ashpeak race goldens only. They were recorded on main's
  physics; on round 12 the vehicle sim differs (first mismatch at tick 240 in each) and the
  header carries contract 0.3.1. Before blessing: all 8 `pick_*.ron` **PASS** their
  expectations (`simcli run`), and `race_picker` (release) races every Ashpeak race to the flag
  on round 12 physics with the new police profiles (15 / 15 pass).

## Gate (env `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0`)

Final, on the merged tip (`a04af158`):

| Step | Exit | Notes |
|---|---|---|
| `cargo fmt --all -- --check` | 0 | |
| `cargo clippy --workspace --all-targets -- -D warnings` | 0 | |
| `cargo test --workspace --no-fail-fast` | 0 | 1602 passed, 0 failed, 42 ignored |
| `cargo build --target wasm32-unknown-unknown -p wp_wasm` | 0 | |
| `cd apps/testbed-web && npm run wasm && npx tsc --noEmit -p .` | 0, 0 | |
| `e2e/street.mjs` | 0 | 105 PASS |
| `e2e/traffic_bodies.mjs` | 0 | PASS (orphan bodies 0) |
| `e2e/race_spots.mjs` (PR's e2e), `vesper_v2` · `CITY=ashpeak` | 0 · 0 | 16 · 16 PASS; prompt shows buy-in / purse |
| `wp_race --test race_picker` (release; on `b5281e40`) | 0 | 15 / 15, every Ashpeak race to the flag |
| `simcli goldens` (release) | 1 | 4 red, **identical on round 12 `5080fa80` itself** (same hashes): `jam_pursuit_grid` (tick 14400), `jam_pursuit_vesper` (tick 12000), `police_tunnel_escape_founders_cut` + `_control` (expectation `police_event_before(kind:12, 95 s)` never; round 12's own notes already list the Founders Cut pair as red). Not from the port, not blessed: for round 12 / E (police row 156 landed in `5080fa80`). All 8 Ashpeak goldens and every other round-12 golden match. |
| `e2e/perf_check.mjs` | 1 | RTF / slow frames / steady alloc fail on round 12 too on this container (machine-bound); every machine-independent gate passes (see Perf) |

Tooling installed in the container for this: `wasm-pack` (cargo install), testbed `npm ci`;
`just` is not installed (justfile commands run directly).

## Perf (same container, round 12 `db0035d9` vs port tip)

Native release, `wp_wasm --example chase_bench hakone_gt 30 [city]` (the same `Session` the
browser runs), two runs each; browser `apps/testbed-web/e2e/perf_check.mjs` (desktop 1280×720
gates) and `e2e/frame_profile.mjs` (`CITY=ashpeak`), headless SwiftShader on this 4-core
container.

| Measurement | round 12 `db0035d9` | port tip | Budget |
|---|---|---|---|
| Native city chase, Port Vesper: sim ms / 60 Hz frame (mean · p95) | 3.48–3.59 · 5.3–6.3 | 3.37–3.65 · 4.9–5.6 | — |
| Same, final state hash | `b987092867952e80` | **`b987092867952e80`** (identical: Port Vesper sim unchanged) | — |
| Native city chase, Ashpeak map (22 → 20 vehicles, 4643 props) | — | 3.54–3.74 · 5.8–6.5 | — |
| Desktop chase (Vesper): draw calls · triangles | 425 · 326.6 k | 437 · 329.0 k | < 600 · ≤ 400 k |
| Desktop rain chase (Vesper): draw calls · triangles | 411 · 308.5 k | 415 · 304.3 k | < 600 · ≤ 400 k |
| Desktop chase on the Ashpeak map (`frame_profile`, `CITY=ashpeak`) | — | 432 · 336.8 k (city 149 calls, 4679 tiles / 128 drawn) | < 600 · ≤ 400 k |
| Phone chase 740×360 (informational, row 145): draw calls · triangles | 327 · 352.3 k | 331 · 348.2 k | (300 · 400 k, not a gate) |
| Browser sim section, chase · rain, ms per rendered frame | 91.5 · 101.6 | 85.1 · 108.9 | (noise ±10) |
| perf_check RTF / frames > 250 ms / steady alloc | 0.76, 0.68 / 28, 28 / 587, 640 KB | 0.69, 0.64 / 29, 29 / 572, 685 KB | 0.9 / 12 / 200 KB |

RTF, slow frames and steady allocation **fail on round 12 too** on this container (SwiftShader
raster ~230 ms of the frame; `browser-perf.md` calibration): not from the port. Draw calls
+4…+12 on Vesper are the honk-spot markers near the spawn (ring, disc, pillar, arrow, label
per spot; frustum-culled). Every desktop gate that is machine-independent passes.

## Collisions with the parallel ports

- **PR #49 touge** (`claude/project-thread-rgdat5`): also edits `wp_race/src/catalog.rs`,
  `lib.rs`, README, `wp_wasm` `lib.rs` / `race.rs` / `scenario.rs` / `session.rs`,
  `wp_world/src/lib.rs`, `gen_city.rs`, simcli `data.rs` / `scenario.rs`, testbed `main.ts` /
  `sim.ts` / `layout.ts` / `index.html`. Its sealed touge maps (`World::touge`) overlap in
  theme with Ashpeak's Crowfoot Pass touge races (single-file grids, `RaceDef::grid`): pick one
  "touge" vocabulary; its new races also need `data/police/races.toml` tables.
- **PR #47 city betting** (`claude/project-thread-7fsvk6`): edits `catalog.rs`,
  `race_picker.rs`, `wp_wasm` `race.rs` / `race_tests.rs` / `session.rs` / `lib.rs`, `main.ts`,
  `sim.ts`, `race.md`. Its countdown bets (`race_place_bet`) apply to spot-started races too
  (they are picker races); its five new city races need police profiles, and spots appear for
  them automatically.
- **PR #46 career**: no file overlap with PR #50.
- **D / ws/world PR #38** (river, canals, bridges, landmarks): coordinated directly; D lands
  after this branch, takes road format v7, and regenerates all three greyboxes. Note: the
  `ashpeak` map contains Port Vesper, so the river etc. grow the Ashpeak greybox further.

## Contract change requests

| # | Change | Why | Who | Status |
|---|---|---|---|---|
| 1 | `DistrictId::Farm` (or `Rural`) | Harlan Flats uses `DistrictId::Suburb` (traffic / police density and economy read it as suburb) | 05, 06, 08 | proposed (from PR #50's known gaps) |

## Open questions for the lead

1. Greybox over the preview cap: which option (tiles / coarser ground / skip in preview)?
2. Spot money: I used the picker race's existing buy-in / purse. If "its existing betting"
   meant the street-race wager (one opponent, even money), spots would need a street-style
   offer card instead; say so and I'll switch.
3. The 8 police profiles are proposals; route to E (police) for tuning?
4. Names flagged but kept (Harlan, Crowfoot): rename?
