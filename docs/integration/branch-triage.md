# Branch triage (lead helper, 2026-10-21)

Branch `lead/branch-triage`, from `origin/lead/round12` @ `4ab9703d`. Scope: the `origin/claude/*`
branches whose commits never reached round12. Method per branch: `git cherry` against round12
**and** origin/main (a commit whose patch is on main belongs to `lead/main-sync`), a diff by file
and reading the code where both sides touched the same thing; last push time (< 20 min before
05:00 UTC = maybe still active, left alone). Merges resolve conflicts keeping round12's behaviour.

`lead/*` and `ws/*` branches are out of scope (lead / workstream branches with their own owners).

## Summary

| Branch | Status | Why / where |
|---|---|---|
| `adoring-meitner-dnlwbb` | **owned elsewhere (lead, on round12)** | Owner decision (2026-10-21, "add them all"): Marten 2.0 / Brigand 400 / Talus 2.0T / Tumbril 2.0 are wanted next to compassionate-mccarthy's four (I had first dropped them as same-archetype duplicates). The lead merged them into round12 directly. |
| `compassionate-mccarthy-o60ka7` | **already in (lead merged it into round12, 387a4d3e)** | Same tip as my merge a8d676a6, with my ROSTER/VALIDATION/cars.md resolution; round12's copy taken when round12 was merged back here. Follow-ups on this branch for all eight new cars: capability rows (`wp_opponent` `capability_table`, missing on round12 too). Round12 fixed their shift times / recorded packs (2a9179da) and regenerated their meshes in format 3 (5d360c7d). |
| `detailed-car-models` | **waiting for lead/main-sync** | Built on origin/main (13 of its commits are main's stat bars / pro driver), multi-session (pulls in loving-babbage's traffic styles), mesh format 2 for every car, multi-model police / traffic. Lead decision: merge it here once main-sync has landed on round12, renaming `sentinel_harrier` → `sentinel_vigil` ("Sentinel Vigil": "Harrier" is a real model name) everywhere and regenerating the 8 new roster cars' meshes in its format. main-sync had not landed when this pass ended (round12 29 commits behind origin/main). |
| `trusting-mendel-ta85bb` (fix) | — | Its `wp_render` mesh reader only accepted mesh format 1; round12's meshes are format 3 (formats 2/3 only add the optional buckling binding), so every mesh failed to parse. Reader now takes 1–3 and ignores the buckling data. |
| `fervent-cerf-ft2h9h` | **merged (adapted)** | Photo mode. F3 became the camera cycle (owner request 2026-10-21) → photo mode opens with **F7** / the photo button; gamepad Start is the pause menu → it no longer enters photo mode (Start / B / Esc / F7 still leave); game actions are skipped while photo mode is active; `layers.hide` sits next to round12's `layers.suppress`; audio ducking next to `setPaused`. Gap: photos bypass the low-poly art style's post pass. |
| `hopeful-cray-0dtq2z` | **merged (adapted)** | Bridge decks occlude line of sight (all sightlines, not just the helicopter's; overpasses were not covered by round12's tunnel/trench spans), helicopter eyes-on / lost-visual / position radio (+2 `tools.toml` keys), debug launch, native Vesper tracking tests, airframe + spotlight on round12's existing `updateHelicopters` path (spotlight is an unlit cone + ground pool, no real `SpotLight`: round12 keeps a fixed light count), siren pitch-curve 50 ms lead. Dropped as duplicate: its sight-height fix (round12's `eye_height_at`, af853e24). Goldens unchanged. Spotlight look not checked in a browser. |
| `dreamy-bohr-2lm4wv` | **superseded** | A parallel recorded-audio pipeline (`tools/audio-packs/build.py`). Round12 has its own (`data/audio/sources.json` + `LICENSES.md`, world_rec with 36 cues vs this branch's 28, engine_rec_*, siren_rec, radio_rec); merging would delete round12's. Only leftover: *recorded* turbo whistle (freesound 496171) and supercharger whine (freesound 529315) clips — round12 deliberately voices both with the synth ("no recording found"); a lead for the audio owner, not ported here. |
| `serene-johnson-xfyckq` | **merged (ported)** | Tire squeal, ported onto round12's tire model (triage/squeal-port): squeal normalised by each car's own tyre peak slip, braking lock-up timbre (lower pitch), resonator squeal synth in the testbed. Skipped as superseded: attack/release keys (round12's `follow()` / floors), wheelspin timbre (round12's separate spin cues), its pitch formula, speed-tracking roar (round12's two-stage roar), the voices.ts fade fix (round12's playtest #19 fix). Playtest row 128 (was its row 33). |
| `stoic-bohr-ecqmn5` | **already in** | All three commits are on round12 as e5ea6a37 (sirens sweep smoothly), 0f10b6fd (`?city=`), 19462ef4 (grass turf). |
| `happy-lovelace-4e2oo8` | **merged** | Suspend audio while paused (`audio.setPaused(app.paused)` added to round12's frame loop). |
| `trusting-mendel-ta85bb` | **merged** | New crate `wp_render` (own wgpu renderer spike, time of day) + testbed-native `gpu` feature. wgpu / pollster / png are optional behind `gpu`; wp_render is not reachable from wp_wasm. Adds a workspace member — lead to note it in the crate map. |
| `funny-hypatia-wq2ojr` | **merged (cherry-picked + ported)** | Built on main; cherry-picked only its own commits (not main's stat bars): wp_economy `visuals`, simcli entity `mods`, wp_wasm mod-visual exports, docs; stat-bar lines from main stripped from the conflicts. The testbed commit 87d95f2a was ported onto round12's batched panel renderer (triage/modshop): mod parts are skinned parts with their own per-material batches (dent / detach with their region), extra shared materials, per-car copies for paint finish / tint / trim (the original edited shared materials, recolouring every car), looks re-applied on art-style switches; the crash-replay `Session` copies the build. Mirrors now get vinyl too (they're in the paint batch). |
| `festive-dijkstra-5rxjnw` | **owned elsewhere (lead/main-sync)** | Its grid-city viaduct + diamond ramps (aed96621, d91fb6f6) and Loop 9 highway (674efa72, e0b61729) are all on origin/main; nothing unique left. |
| `awesome-faraday-epejyl` | **merged** | Wider race gates (min 26 m), off-course racers reset onto the route (published to the owner as the 0.12.25 race build). Docs: round12 and this branch both made a "0.12.25" and a playtest "row 120" from the same commit; both kept, this branch's row renumbered **127** (round12 later used 126 too). **Heads-up for main-sync:** main's 94ffb44c ("leaving the route is never a DQ", owner call) deletes the corridor clock this reset is built on, and this branch still DQs when reset is off (server races). Suggested resolution: keep the reset (driven by the corridor clock), drop the `LeftCorridor` DQ path. |
| `serene-bohr-cey1t7` | **merged** | Race judge remembers every police id (retired-unit bump ≠ civilian hit), wp_net sends events missed while reconnecting, testbed allocation pass (the "WIP" commit is completed by the next one). |
| `happy-knuth-g39ogc` | **split** | Siren yielding (1f98243f) **already in** (round12 reimplemented it as the `wp_world` siren module: yield phases, pull-over, oncoming / near, d230a5ef / 738d2b2b). Stuck-cop fixes (a5c0b5bb, 02181f6b, 5fdc619e) → **police session E**: they collide with round12's push-through / queue rule / wall feelers and with the unstick work in loving-wright / quirky-fermi that E is reviewing. Loop 9 freeway (e53ee2a6, e9c48a44) → **lead/main-sync** (main has festive-dijkstra's Loop 9 highway; one of the two). Turn signals (fcc9ea35) → **lead/parked-signals**. Bless d3c8c484 / playtest rows cf71fee3 go with those. |
| `lucid-cori-8sjuzz` | **already in** | Was still active at triage start (pushed 04:48); since then round12 has both commits: b6dfc12d (solid terrain = drawn terrain) and 8abeacc6 (drawn terrain follows the ground, row 119 follow-up = its ec1b7f35). |
| `optimistic-wozniak-13v80v` | owned elsewhere | Dentable parked cars → lead/parked-signals. |
| `beautiful-bell-x1ijjj-stiff-ride` | superseded | lead/grip-balance (one-version merge). |
| `beautiful-bell-x1ijjj` | owned elsewhere (police E) | Note: besides the pro driver (on main) it has **33e42c3a "data/tires: grass grip matches dirt"** + bless 0058db44 — a tyre change on no integration branch; for lead/grip-balance to decide. |
| `optimistic-allen-qsfyuj`, `inspiring-hamilton-6096s9`, `loving-wright-6zjxx4`, `affectionate-knuth-3xlkwf`, `quirky-pasteur-qjldcq`, `dazzling-mayer-ovag76`, `beautiful-hopper-sfuta0` | owned elsewhere (police E) | quirky-pasteur is only main's pro-driver commits. beautiful-hopper was pushed 04:49 (active). |
| `loving-babbage-c9mjmf`, `epic-keller-nck8h5` | owned elsewhere (lead/main-sync) | Only main's commits, except epic-keller's c941e3f0 (stat bars on the physics rig — stat-bar stream, goes with main). loving-babbage is also merged into detailed-car-models. |
| `quirky-fermi-f4mvd2` | owned elsewhere (main-sync + police E) | Main's commits plus police work not on main: f9ebb4e0 (road graph centreline), 5b3a2de6 (terrain-aware sightlines, engage on witnessed violations, back out when pinned), 13a58aa4, bless 076cff6e, docs 3be54f00 → E. |
| `upbeat-faraday-b7qcqe` | owned elsewhere (main-sync) | Main's commits plus **4a4efba1 "wp_vehicle: high-speed catches without snap oversteer"** + bless b9a81abe + docs 1fdc4750, on no integration branch (not on grip-balance either) — handling, for lead/grip-balance to decide. |
| `charming-curie-3vttop`, `magical-goldberg-k0vjje` | **dropped (stale history)** | 2026-10-19, no common ancestor with round12 (pre-rewrite history of the whole project); their work reached round12 through earlier lead runs. |
| `affectionate-faraday-2y99m2`, `keen-bardeen-ej8cc0`, `laughing-newton-0xqr6q`, `loving-tesla-bpkq1m` | already in | No commits outside round12. |

## Gate

Env: `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0`, 4 cores, other
sessions building (load 10–13 most of the time). wasm-pack is not installed here: the testbed
package was built with `cargo build --release --target wasm32-unknown-unknown -p wp_wasm` +
`wasm-bindgen 0.2.129 --target web` (same as `npm run wasm` minus wasm-opt).

| Run | fmt | clippy -D warnings | cargo test --workspace | wasm32 build | tsc | goldens |
|---|---|---|---|---|---|---|
| baseline round12 `4ab9703d` | ok | ok | 1 FAIL: `wp_server` e2e `client_joins_drives_and_admin_reports_it` (inputs_missing under load; passed in both later runs) | ok | — | ok |
| gate 1 (groups 1–3 + wp_render + mods + squeal) | ok | ok | 5 FAIL, all from the new cars / wp_render: car-assets mesh format 1 (expected 3), wp_audio recorded packs, wp_vehicle stock shift times, wp_opponent capability rows, wp_render reader format 1 only | ok | ok | ok (no golden moved) |
| gate 2 (+ round12 39321caf, helicopter, fixes) | ok | ok | ok | ok | 1 FAIL → fixed (prop chunks lacked round12's indicator geometries, 223bb358) | ok (no golden moved, no bless) |

No `[bless]` commit on this branch: no golden hash moved.

Browser e2e on the gate-2 head (+ 773e4021):
- `e2e/photo.mjs`: 22 PASS after 773e4021 (round12's capture-phase Esc → pause menu swallowed photo mode's Esc; the "paused before" step used P, which opens the pause menu since round 12).
- `e2e/helicopter.mjs`: all PASS (tracking with visual, 120 m above the car, spotlight on it).
- `e2e/run.mjs`: 217 PASS, including the mod shop checks (4b) and the helicopter block. FAIL: `perf_check` (see below) and `reset.mjs` "ghost blinks" — reset.mjs passes 9/9 when re-run alone (load).
- `e2e/perf_check.mjs`, same machine back to back (software GL, low load): round12 39321caf and this branch fail the **same 9 checks** (real-time factor ~0.5, frames over 250 ms, steady allocation > 200 KB/frame). Steady allocation per frame, round12 → this branch: chase 435 → 372 KB, rain 606 → 369 KB, phone 419 → 374 KB (serene-bohr's allocation pass); draw calls / triangles / lights / wasm calls within budget on both.

## Still open (for the lead)

- **detailed-car-models**: merge after lead/main-sync lands (rename `sentinel_harrier` → `sentinel_vigil`). Note: its meshes are format 2, round12 / this branch are on format 3 (crumple binding), so "regenerate in its format" means regenerating its cars with round12's `tools/assets` (format 3), not downgrading.
- **awesome-faraday vs main 94ffb44c** (corridor clock removed on main): for main-sync, see the row above.
- **Physics on no integration branch:** beautiful-bell 33e42c3a (grass grip = dirt) and upbeat-faraday 4a4efba1 (high-speed catches) → lead/grip-balance to decide.
- **Police for E:** happy-knuth a5c0b5bb / 02181f6b / 5fdc619e (stuck-cop), quirky-fermi f9ebb4e0 / 5b3a2de6 / 13a58aa4 / 076cff6e / 3be54f00.
- **Audio lead for J:** dreamy-bohr's recorded turbo whistle / supercharger whine clips.
- **Unverified visually:** helicopter spotlight look (unlit cone, pool opacity 0.18); photo mode skips the low-poly post pass.
- `wp_render` is a new workspace member (crate map in docs/INTEGRATION.md not updated: lead-owned).
