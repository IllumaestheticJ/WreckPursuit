# Branch triage 2 (lead helper, 2026-10-22)

Branch `lead/triage-2`, from `origin/lead/round12` @ `2bac2382`. Scope: the unmerged
`origin/claude/*` branches the lead listed (owner: "what other existing, unmerged branches are
there? start merging those as well"). Method per branch: `git cherry` against round12 (round12
contains all of origin/main), the branch's own commits read against round12's current code,
`git merge-tree` for conflicts. Ported commits are cherry-picks (`-x`) resolved onto round12's
code, keeping round12's behaviour where both sides touched the same thing.

Owner rules that win over any branch (not re-litigated here): one physics for all cars and both
modes; the shipped handling (no handling / steering / ride / tyre change without the lead);
AI racers never give up; police rows 131 / 133 / 140; big engines procedural audio; format-4 car
meshes; no real car names; CLAUDE.md determinism rules.

## Summary

| Branch | Decision | Why | Commits taken |
|---|---|---|---|
| `sharp-hypatia-09djnu` | **merged** | Insurance covers damage taken while wanted; only a capture voids it (`sustained_while_wanted` → `sustained_before_capture`). Ported onto round12's career module, which the branch predates (career view field `damage_before_capture`, garage warning text). Old saves: the old field is ignored (no `deny_unknown_fields`), i.e. old "while wanted" flags clear. Gameplay rule change — confirm it was the owner's call. | merge 301e8e3e + port e1d5d2c5 |
| `determined-mccarthy-0jovkk` | **ported** | Garage POIs open repairs: stop (< 3 m/s, no pursuit) in a shop → G's `RepairAll` quote at that shop, Repair / Leave; markers (ring + beam, blue legit / amber crooked) and a panel in the testbed. Adapted: off in picker races (a stop would hold a racer) and career runs (the career garage owns repairs and the account); crash-replay sessions carry no garages; panel element `#ui-garage-visit` (round12's career garage already uses `#ui-garage`). Outside the sim step: hashes unchanged. | 082273c1, 1d0f5efe (+ 37dc26dc) |
| `gifted-gauss-y4n7mo` | **held (ported, then reverted)** | Slipstream (`Sim::set_drag_scale`, `draft::slipstream`, `AiEntrants` with `RaceConfig::draft`). On round12's tip it makes `wp_race race_picker::hollins_hill_sprint` never finish (passes without), and its own `draft_lets_a_faster_driver_in_the_same_car_pass` shows no effect on the shipped handling (10 places gained with and without). Aero drag during the physics freeze (row 148) → physics lead. Note if re-taken: the browser's races must apply the drag too (the port did, in `wp_wasm::race::RaceRun::pre_step`), or the AI sits in a tow that does nothing. | none (14ab267c + db45e4a2 reverted in cd46671b) |
| `beautiful-einstein-b1noth` | **pier fix ported; rest needs a lead/owner decision** | 823f87a8 (no bridge piers on / within 1.5 m of a road under a deck) re-applied on round12's `glb.rs` + test; render only (piers have no colliders), greyboxes regenerated. The rest rewrites the shipped v2 Vesper map (river, canal, 8+3 bridges, terrain holes) and the grid's highway outer walls → 10 goldens; its "road format v4" clashes with round12's (round12 is at v5); its Hollins Tunnel duplicates round12's covered spans. | 823f87a8 (ported) |
| `sharp-goodall-y8gbuc` | **needs lead decision** | Port Vesper **v3**: a whole new in-development city (`wp_world::metro`, ~30k lines, airport / docks / rail / parking / buildings / scenery, static feature colliders, 28.7 MB greybox). Doesn't touch v2 / grid output, but its road "format v4" `features` field must become v6 on round12 and is type-incompatible with einstein's. Its pier fix is the same idea as einstein's (taken). Merge as a branch (internal merges), ~16 conflicting files, if wanted. | none |
| `nifty-gauss-k1pgb5` | **needs lead decision** | Backyard fences / side gates / perimeter walls on v2 Vesper. Round12's `residential.rs` already lays out lots with front / side fences and hedges; this hooks the old `vesper.rs::buildings()`, adds its own `Fence` type (round12 has `DecorKind::Fence`) and a format bump, and adds solid colliders to the shipped map (goldens). Worth re-implementing as round12 `Decor` from residential lots if the owner wants rear walls. | none |
| `lucid-cori-8sjuzz` | **held → police owner (ported, then reverted)** | Re-pushed since the first triage. Road-network centrelines follow the road centre, police route corners round into drivable arcs — neither on round12. With 29c237f0 / 9d4a508c / 1285571f, `wp_opponent race::live_police_chase_ends_in_escape_or_capture_without_stuck_cars` fails (seed 4: a busted racer still moving 6 m/s); the branch only got past it by moving the seed to 1 (551a7145, its own note: seeds 1–8 fail 3 of 8 after the change). Not taken as a seed re-aim. a423a0ea (patrol back-out) is covered by round12's `stuck.rs`. | none (reverted in 1d9655ad) |
| `inspiring-hamilton-6096s9` | **held → police owner (ported, then reverted)** | Owner complaint "civilian reports far too soon": an unseen violation needs a civilian caller with line of sight nearby (`heat.caller_range_m`). On round12 it stops unseen speeding adding any heat in the city chase (`wp_wasm city_chase_speeding_player_gets_heat`: 2 violations, 0 heat) and its own test finds no quiet spot on round12's maps. Needs a rework on round12's police (callers from round12's traffic / parked cars). | none (reverted in 5a462d31) |
| `charming-mayer-k7ozj6` | **ported** | `PoliceSetup::hazards` (statistical traffic-hazard stand-in) defaults to off; wp_sim / simcli already set it explicitly, so only default-relying callers (tests, netsim) change. | 76306d6e |
| `funny-hypatia-wq2ojr` | **already in** | Its last commit (`PartKind::regions` doc) is on round12 (cherry-pick came out empty); the rest was ported in triage 1. | — |
| `trusting-babbage-xtzw9n` | superseded | Sightlines from eye height above the ground = round12's `eye_height_at` (wp_police lib.rs). | — |
| `quirky-fermi-f4mvd2` | superseded | Terrain-aware sightlines (`eye_height_at`), witnessed violation → pursuit (row 80), contact → `on_assault`, back-out (`stuck.rs`); its road centreline fix duplicates lucid-cori's (taken). | — |
| `loving-wright-6zjxx4` | superseded | U-turn / three-point turns = round12's `stuck::Phase::UTurn/Swing` + chase routes that turn round. | — |
| `optimistic-allen-qsfyuj` | superseded | Round12 has its own radar (`clock_speeders`, `RadarConfig`) and caught-in-the-act (rows 60 / 80). | — |
| `nifty-lovelace-mqhigm` | superseded | Aggression by wanted level = round12 `[aggression]` (row 63); model by heat = round12's `car_mix` police fleet (L4–5 sentinel_vigil). The Wolfhound PX car is a format-1 mesh clone of the old Wolfhound; not needed with car_mix. | — |
| `happy-knuth-g39ogc` | superseded (one gap noted) | Siren yielding, turn signals, Loop 9 freeway and stuck-cop unsticking are all on round12 in other forms. Gap still open on round12: parked-car props / pulled-over civilians are not reported to police avoidance (02181f6b + the `parked_nudge` part of a5c0b5bb) — for police session E. | — |
| `affectionate-knuth-3xlkwf` | **needs owner decision** | Strategic containment (escape points, posts ahead, tail units, widening search): 1.1k lines, not on round12; its reinforcement spawns would have to go through round12's out-of-view spawn rules (rows 131 / 133 / 140). | none |
| `fervent-bohr-y33ku8` | **needs owner decision** | Bounty bonuses (cop wrecks, roadblock / spike dodges) wired into the economy — not on round12 (only `cops_disabled`) — mixed with a new helicopter flight model (fuel, phases) that would replace the shipped P9 helicopter. Design / economy call; heavy conflicts. | none |
| `bold-ride-zezrp2` | **not taken (frontend port possible)** | Helicopter model / spotlight / call signs / pilot call-outs are on round12 (P9, "Air N"). Missing on round12: rotor + pilot-radio audio, minimap heli icon with tether, chase-HUD radar, scanner AIR badge, and a +12 % helicopter loudness with no owner row. A frontend-only port (drop its wp_police / wp_wasm parts) for the testbed/UI owner. | none |
| `beautiful-hopper-sfuta0` | **needs owner decision** (balance) | Street-spec power rebalance (~240 W/kg, every car json + lattices), police caps ×1.5 and `physical_accel_mps2` (round12 has its own `[chase] cap_scale` aggression); its AI / stuck fixes are superseded by round12's `stuck.rs` and parked-vehicle passing. Based on old main: a merge would delete round12 files. 6 [bless] commits. | none |
| `beautiful-bell-x1ijjj` | **held: handling paused** (tyres; physics freeze, row 148) | Grass grip = dirt in every tyre file (a physics change; flagged in triage 1 too). If approved: 33e42c3a only, then re-bless. | none |
| `trusting-fermi-djbpzx` | **needs owner decision**; mostly superseded | Style-sheet looks and format-2 meshes superseded by format 4; mixed fleets by round12's `car_mix`. New and absent: police units Shade / Marshal / Breacher, 4 heavy trucks, `commercial.json` tyres, hitched semi-trailers. Its `sentinel_warden` (prisoner van) collides with round12's `sentinel_warden` (V8 sedan); Courser duplicates `sentinel_vigil`; **"Marshal" is a real model name** (rule 9) — rename if ever taken. | none |
| `dreamy-bohr-2lm4wv` | superseded (pipeline); clips → audio owner | A merge would replace round12's recorded packs and pipeline, and put big engines / police on recordings (breaks 0.12.36 / 0.12.39). Clips round12 lacks, for the audio owner to hand-import into `data/audio/sources.json`: turbo whistle, supercharger whine, exhaust pops, limiter, surface loops, loose scrub, body rattle (7b17ec1d, 5797c608, 23918fa5; CC0). | none |
| `compassionate-mccarthy-o60ka7` | superseded | Stat bars and capability rows for the new cars are on round12; its meshes are format 2 (round12: 4); its audio puts bulwark_400 (V8) on a recording (breaks 0.12.39). | — |
| `stoic-bohr-ecqmn5` | already in | e5ea6a37 / 0f10b6fd / 19462ef4 on round12. | — |
| `serene-johnson-xfyckq` | already in | Ported in triage 1 (887450e6). | — |
| `practical-lamport-ub5qiv` | superseded (parallel design) | A second route planner (`Tactic::Evade`) on the old base; round12's racers already plan round traffic (gap planner, `plan_pass`, lane-splitting, flight line). Only idea not on round12: aggressive drivers evading through the opposing lanes on open roads. | — |
| `optimistic-wozniak-13v80v` | superseded | Dentable parked cars shipped in 0.12.26 (lead/parked-signals). | — |
| `fuzz-barrier-clear`, `epic-keller-nck8h5` | already in | No commit outside round12 by patch id. | — |
| `quirky-pasteur-qjldcq`, `serene-bohr-cey1t7` | **skipped (active)** | Pushed < 60 min before triage start. | — |

## Gate

Env: `CARGO_INCREMENTAL=0`, debug info off, 4 cores. Branch = round12 `5110265a` merged in
(`d4cce03c`) + the ports above.

**Round12's own tip did not compile** (`wp_police`: the race-police profiles merge vs the row-144
radio tiers). Fixed in the merge commit: `race.rs` `RadioTier::Critical` → `Escalation` (the
variant `Critical` is a serde alias of) and `SpikeStrip::between` sets `scripted: false`. The
lead's round12 needs the same two lines.

| Check | Result |
|---|---|
| `cargo fmt --all -- --check` | ok |
| `cargo clippy --workspace --all-targets -- -D warnings` | ok |
| `cargo test --workspace` | 181 suites ok (clean rebuild of every workspace crate); red: `simcli goldens_match` (race_lantern_checkpoint), `wp_race race_picker::{grid_highway_sprint, hollins_hill_sprint_pace_without_police}` — **all three also red on round12 `5110265a`** (+ the compile fix), checked in a separate worktree with its own target dir (a shared `CARGO_TARGET_DIR` across two worktrees of this repo mixes artifacts: Cargo hashes path crates relative to the workspace root). Slipstream's two reds gone with its revert. |
| `cargo run --release -p simcli -- goldens` | 1 differs: `race_lantern_checkpoint` (first mismatch tick 240) — same on round12's tip, i.e. round12's race-police merge, not this branch. **Not blessed here** (not ours to explain). |
| `cargo build --target wasm32-unknown-unknown -p wp_wasm` | ok |
| testbed `tsc --noEmit` | ok apart from the generated `wasm/pkg` module (wasm-bindgen not installed here) |
| ui-kit `tsc --noEmit` + `vitest` | ok, 111 tests |
| Greyboxes | regenerated (`d64d1771`), `shipped_city_v2_greybox_is_current` ok |

No `[bless]` commit on this branch: no golden moved because of a branch taken here.
Not run: browser e2e (garage-visit step in `e2e/run.mjs` unverified in a browser).

## For the lead / owner

- **Round12 compile fix** (above) — take it with this branch or apply it directly.
- **Round12 reds** not from this branch: `race_lantern_checkpoint` golden and two picker tests
  (`grid_highway_sprint`: 1 of 4 finish; `hollins_hill_sprint_pace_without_police`: "police
  profile" unwrap) — from the race-specific police profiles merge (8757f12f).
- **Owner decisions:** Port Vesper v3 (sharp-goodall); river / canal / bridges and highway outer
  walls on the shipped v2 map (beautiful-einstein); backyard fences (nifty-gauss); street-spec
  power rebalance + police caps (beautiful-hopper); strategic containment (affectionate-knuth);
  bounty bonuses + helicopter flight model (fervent-bohr); extra police units / trucks / trailers
  (trusting-fermi; "Marshal" is a real model name).
- **Held: handling paused:** beautiful-bell (grass grip = dirt); slipstream (gifted-gauss) for the
  physics lead.
- **Police owner:** lucid-cori centrelines + route arcs (breaks a race test, see row), the
  civilian-caller rule (inspiring-hamilton, owner complaint, needs a round12 rework), parked
  cars / pulled-over civilians missing from police avoidance (happy-knuth 02181f6b).
- **Testbed / UI owner:** bold-ride's helicopter frontend (rotor + pilot audio, minimap tether,
  AIR badge; +12 % loudness has no owner row).
- **Audio owner:** dreamy-bohr's CC0 clips (turbo whistle, supercharger whine, pops, limiter,
  surface loops, loose scrub, body rattle).
- **Insurance rule** (sharp-hypatia, merged): confirm it was the owner's call.
- **Garage visits** (merged): gameplay addition; legit shops' police reports do nothing yet.
