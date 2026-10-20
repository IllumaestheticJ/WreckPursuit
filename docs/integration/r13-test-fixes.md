# Round-13 test fixes (lead helper) integration notes

- **Workstream / prompt:** lead helper, red tests after round 13 landed on `lead/round12`
- **Branch:** `lead/r13-test-fixes` (from `origin/lead/round12` @ 53f801c, then merged
  `origin/lead/round12` @ ecf4d7b as the lead asked)
- **Crates / folders touched:** `crates/wp_race/tests`, `crates/wp_wasm/src` (test only),
  `apps/simcli/golden` (`[bless]` ×2). No sim code: versus `origin/lead/round12` this branch
  differs only in two test files, the goldens and this doc.
- **Contract version built against:** 0.3.0 (no contract change)

## Summary (on the merged tip, lead/round12 @ ecf4d7b + this branch)

| Test | Status | What was wrong / what changed |
|---|---|---|
| `wp_race` `race_picker::loop9_circuit` | **green** | The test's assumption (≥ 4 finishers in a race with police). Split into a police-free pace test (old expectations) and a police-design test. Not an AI pace regression. |
| `wp_race` `race_picker::loop9_circuit_pace_without_police` | **green** (new) | 5 of 6 + 1 DQ, winner 209.4 s. |
| `wp_race` `race_picker::lantern_checkpoint_draws_the_police` | **green** (no test change) | Red on 53f801c because of a racer↔traffic junction deadlock (section 1). After the merge of ecf4d7b (ws/world, ws/police, police-radio-v2) all 4 racers end by the police, each after ≥ 2 gates. The deadlock analysis below is still worth D's and N's time: it only stopped showing in this seed. |
| `wp_race` `race_picker::grid_highway_sprint` (+ `_is_deterministic`) | **red**, police (E) | New with ecf4d7b: a police unit with its siren on, not in any pursuit, stuck in a three-point-turn loop just past the finish for about 80 s. Civilians yield to its siren in the racers' lane. Section 4. Not AI pace. |
| `wp_wasm` `race_tests::short_sprint_with_ai_runs_to_results_and_pays_through_the_economy` | **red**, police (E) | The same race and the same cause: all 4 autopilot racers held at y ≈ 345–360, 40 m before the finish, from about 60 s to the 120 s limit. |
| `wp_wasm` `crash_replay_tests::race_crash_replays_with_the_judge_and_no_crash_on_the_grid` | **green** | The test setup: it parked a racer that wasn't the one in front of the player (section 2). |
| `wp_wasm` `tests::simcli_golden_replays_play_back_in_the_browser_core` | **green** | Stale goldens. Re-blessed (section 3). |

No `wp_vehicle` change was needed. Steering, ride and assists are untouched. For
`lead/opponent-pace`: the only AI-only finding is loop9's racer 2 (entity 2) being DQ'd
(LeftCorridor) at gate 34, progress ≈ 6,920 m, with and without police on the merged tip.
The loop9 tests allow one racing incident, as the original `check()` did.

## 1. `wp_race` race picker: AI pace did not regress

**How it was checked.** The pre-round-13 base was the lead side of the ws/vehicle merge,
608a50a (before `5849df0 Merge ws/vehicle` and `0620323 Merge ws/police`). I ran the same
tests on it (release, `cargo test --release -p wp_race --test race_picker`). For a pace
comparison without police noise I also ran with police presence 0 (`WP_RACE_PRESENCE=0`).
The opponent AI drives with `AssistProfile::ai()` (countersteer help 0, sensitivity 0), the
preset that A's rows-57 work kept bit-identical in the wheel/AI path.

| Race, police presence 0 | 608a50a (before round 13) | `lead/round12` (round 13) |
|---|---|---|
| loop9_circuit | 5 finish + 1 DQ LeftCorridor; winner **218.4 s**; P2–P5 222.4 / 225.6 / 229.3 / 234.7 | **6 of 6** finish; winner **215.6 s**; 222.3 / 225.6 / 226.9 / 228.9 / 234.0 |
| hollins_sprint | 4 of 4; 62.8 / 66.1 / 77.3 / 79.6 s | 4 of 4; 63.5 / 66.8 / 77.9 / 80.1 s |
| grid_highway_sprint | 4 of 4; 48.9 / 56.9 / 62.3 / 62.7 s | 4 of 4; 54.0 / 61.8 / 62.4 / 63.6 s |
| lantern_checkpoint | **stuck**: all 4 at 3 gates, running at 300 s | **stuck**: all 4 at 3 gates, running at 300 s |

| Race, scenario police | 608a50a | `lead/round12` |
|---|---|---|
| loop9_circuit | **0** finish (winner Fled), 2 Captured, 3 Fled, 1 DQ; 12 pursuits | 1 finish (216.4 s), 1 FinishWindow, 2 Captured, 2 Fled; 9 pursuits |
| lantern_checkpoint | never finished: 2 Running at 0 gates, 1 Fled, 1 Captured | the same |

The AI does not spin or slow down with the new steering. Both race-picker failures predate
round-13 vehicle work and come from police round 12b/13 and traffic. police.md
(round 12b/13), world.md and audio.md already list them as red on `lead/round12`.

### `loop9_circuit` (changed test, green)

Owner rule: races with police can end in captures. So "≥ 4 of 6 finish" with an L2+ patrol
called to the meet no longer measures pace. It measures how much police round 13 catches. The
test is split. Its pace check is kept at full strength:
- **`loop9_circuit_pace_without_police`** (new): the same lap with police presence 0 and the
  old expectations, unchanged (`check(&r, 150.0..420.0, 4)`), plus no pursuits. Now 6 of 6,
  winner 215.6 s.
- **`loop9_circuit`**: asserts the police design, like `lantern_checkpoint_draws_the_police`
  already does:
  - the patrol is called;
  - the race is judged to the end and nobody stays `Running`;
  - every racer passed ≥ 1 gate;
  - every racer ends `Finished`, `Captured`, `Fled` or `FinishWindow`;
  - any finisher's time is within 150–420 s.

  `FinishWindow` is allowed because the one racer outside the window (46 of 51 gates) was in
  a field with 9 pursuits. **Lead:** tighten this if police should never hold an uncaught
  racer past the window.
- `run_race_with(id, max_s, presence)` overrides the scenario's police presence.
  `run_race(id, max_s)` is unchanged. The `WP_RACE_*` environment knobs still apply on top.

### `lantern_checkpoint_draws_the_police` (red on 53f801c: a real deadlock; green after merging ecf4d7b)

*Status after merging ecf4d7b: passes unchanged (seed 31). With police: 4 pursuits, all four
racers end Captured or Fled after 2–5 gates, race judged at 103.9 s. The analysis below is
from 53f801c. The deadlock pattern is a code problem the new police and traffic only route
around in this seed, so the requests stand.*

I didn't loosen this test. On 53f801c it fails because the race really does deadlock: two racers sit at
0 m/s for the remaining ~4.5 minutes. Tracing seed 31 in release (`WP_RACE_TRACE`, plus a
temporary per-racer tactic/limit dump and traffic-agent dump, removed again):

**Without police** (presence 0, same on 608a50a):
1. From about 55 s, at the junction by Central Precinct (≈ (100, −147); the catalog comment
   already says "the AI jams at the turn by Central Precinct"), civilian **agent 20 / entity
   13** is turning left from connector 1103 into the northbound lane. It stops mid-turn in the
   box at (101.2, −145.3), yaw −0.39, with its tail across the southbound lane.
2. The leading racer (`k1`, entity 3) is stopped right behind that tail at (99, −143), limited
   by `Obstacle(13)`.
3. The civilian waits for the racer, the racer waits for the civilian. The other three racers
   queue nose to tail behind. Their pass checks count "no room" more than 8,000 times.
4. Traffic state of agent 20 for the whole wait: `wait: Line`, holds `permit 1103`,
   `plan [314, 1427]`, `stuck_s` climbing past 39 s, no manoeuvre, `tv_block` clear for both
   planned segments. So it is *not* the tracked-vehicle-in-path check (that gives `Queue` or
   `Obstructed`) and not a siren yield. It is one of the `Bind::Line` rules inside the
   connector. My best candidate is "approaching a short lane that ends at a junction: respect
   its stop line early" (`traffic.rs`, the `seg.is_connector() && ns.length < 40` branch):
   lane 314 is followed by connector 1427. Two northbound cars (agents 87, 96) also wait with
   `Line` for 1421 → 314, so this looks like a short-link double-junction deadlock with the
   stopped racers as the third party.
5. Two experiments, both reverted, both made no difference:
   - **(a)** `wp_opponent`: let a racer stopped > 6 s behind a standing non-racer car creep past
     it with a 0.1 m margin. The civilian's tail physically overlaps the racer's band, so there
     is nothing to creep past.
   - **(b)** `wp_world`: a footprint-sampled version of the tracked-vehicle "in my path" test.
     The civilian isn't held by that test (see 4).

**With police** (the test's configuration): the first junction (≈ (0, 152)) is already
gridlocked by about 10 s. Civilians 22, 9 and 5 are in the box; agent 9 had stopped to yield
to the siren of police 9, which was arriving for the meet. The leader backs out at about 25 s
and flees. Then:
- The patrol pin-captures racer 3 (entity 3) while it is stuck in that jam. As `Busted` it
  parks **where it stands**, mid-lane at the stop line (`Park` sets `n_cmd` = lane edge, which
  is 0.24 m there).
- The fled racer (entity 2) is captured later and parks inside the junction box at (−2, 148).
- Racers 4 and 1 queue behind the captured car for the rest of the race. They have no way past
  ("no room" 1,500+ times) and nothing ever moves them, so the judge never ends the race.

**What has to change (requests, not done here):**
- **D (`wp_world` traffic):** a car that holds its connector permit must not stop *inside* the
  box with `Wait::Line` indefinitely. Either don't grant the permit until the short exit lane
  plus the next junction can take it, or let a car stuck in a box clear it. Repro:
  `WP_RACE_PRESENCE=0 cargo test --release -p wp_race --test race_picker --
  lantern_checkpoint_draws_the_police --nocapture` (stuck from about 55 s at (100, −147)).
- **N (`wp_opponent`):**
  - (1) A `Busted` or `Retired` racer should pull over clear of junctions, the way `Yield`
    already does ("a car stopped mid-lane blocks everyone behind it"), instead of parking on
    the spot.
  - (2) A racer held for a long time by a car that will not move (pass impossible,
    `held_by_car`, so the stuck → `Reverse` rule never fires) needs a way out: back off and
    re-plan, or retire from the race with a DNF. Today it waits for ever. The judge has no
    "stuck" DNF, so the race never ends.
- **E (`wp_police`):** the pinned-capture rule (4 m for 6 s with a unit within 15 m) captures
  racers who are only stuck in a civilian gridlock. That is fine as a rule (owner: captures by
  design), but combined with N(1) above the captured car blocks the road for good.
- **L (race):** the test passes unchanged on ecf4d7b. If D's fix is far off, the race catalog could move the Lantern course off the
  Central Precinct turn (catalog comment). I didn't do that here because it would hide the
  deadlock rather than fix it.

## 2. `wp_wasm` crash replay: the scripted rear-end did not connect (setup fixed)

`race_crash_replays_with_the_judge_and_no_crash_on_the_grid`:
- The player's car (lindwurm_4t, AI assists, throttle only, no steering) launches behind the
  field and from about 7 s runs **nose to tail with AI racer 3**: contacts every tick at
  0 m/s relative speed, and the 8 m/s stretch in the trace. The pre-merge base does the same.
- The test then parked `entities()[1]` (racer 2) 18 m ahead. With the round-13 ride and
  steering, the player's path differs by about 0.8 m sideways and racer 3 stayed between them.
- So the player only shunted racer 3 at a low closing speed. Crash detector input: peak
  `rel_speed` 4.7 m/s against `min_closing_mps` 7. At 8–18 m the same happened. No crash.
- On the base the player happened to clear racer 3 and hit racer 2 1.4 m off-centre (closing
  16.9 m/s), so the old setup only passed by chance.
- **Fix (setup, threshold unchanged):** park the AI racer nearest the player, which is the one
  it is actually behind. Result: square front hits, closing 19.7 / 22.2 / 23.4 m/s and
  Δv 10.3 / 11.3 / 12.0 m/s at 12 / 18 / 24 m. The test keeps 18 m.

## 3. Goldens (`[bless]` e612750, then bb87490)

### First `[bless]` on 53f801c (e612750)

`simcli_golden_replays_play_back_in_the_browser_core` failed on `golden_tbone`. The cause is
more than one stale golden: merge 5849df0 (ws/vehicle into lead/round12) kept lead's golden
hashes and dropped A's round-13 `[bless]` (4b56578). On `lead/round12`, **49 of 82** goldens
mismatched. Even single-car goldens (`golden_barrier_56kmh_arcade`, `golden_tbone`) split
at tick 1–2, which is the vehicle model.

- Rebuilt release `simcli` and ran `simcli run apps/simcli/scenarios`: **81 of 82 pass**.
- `simcli goldens --bless`, then committed all of them except the failing scenario. The
  per-golden reasons are in the commit message: vehicle model only for crash, damage, panels,
  audio and single-car goldens; vehicle model plus police round 13 for police, race and
  weather pursuit goldens. Three scenarios that had never been blessed got goldens:
  `city_tunnel_{founders_cut,hollins}_drive_through`, `police_tunnel_escape_founders_cut_control`.
- After the bless: `simcli goldens` mismatches only on the unblessed scenario below, and the
  browser-core golden replay test passes.

**Not blessed in e612750 (scenario failed on 53f801c; fixed upstream since, see below):**
`police/tunnel_escape_founders_cut.ron`. The racer is
`Captured` at 133.8 s; it expects `first_pursuit_end(outcome: Escaped, 105–150 s)`. Owner:
E/lead. police.md (round 12b gate) lists it as failing on `lead/round12`, "ground units now
reach the racer parked in the Cut sooner". It has no golden, and I didn't add one.

### Second `[bless]` after merging `origin/lead/round12` @ ecf4d7b (bb87490)

lead/round12's own `[bless]` 3c238cc was followed by merges that change the sim:
- 9daf91b ws/damage (Arcade tuning);
- 80c07fc ws/panels (meshes);
- 7834577 ws/testbed (`wp_sim` physics);
- 0bd1457 ws/world (traffic, density, parking);
- 93ffe3c police-radio-v2 and c79c9fe ws/police (dispatch, pursuit, unit).

So 45 goldens mismatched on the tip. I resolved the golden merge conflicts by taking
upstream's files, then ran `simcli run apps/simcli/scenarios`: **85 of 86 pass**. Then
`goldens --bless`. Each merge's effect is in the commit message.

**Not blessed:** `city/siren_traffic_no_siren.ron`, `civilian_collisions` 1 (want 0). It fails
on `origin/lead/round12` itself (this branch has no sim changes), and its golden stays at
lead/round12's version. Owner: D. `police_tunnel_escape_founders_cut` passes again after
b2b8845 re-stated it, so it is blessed now.

## 4. Grid highway sprint: a police unit in a turn-round loop blocks the finish (E)

Merged tip, release, seed 7, `race_picker::grid_highway_sprint`:
- **With police:** winner 125.9 s and one racer never finishes, so the race is never judged.
  On 53f801c the winner took 54.0 s and the test passed.
- **Police presence 0:** winner 60.0 s, 3 finish plus 1 DQ, test passes.

Trace (temporary dumps of vehicles and agents around (6, 370), removed again):
- From about 50 s, police **entity 40** (siren **on**, no racer in pursuit: `pursuits 0`) is at
  the junction just past the finish, ≈ (0, 380).
- For about 85 s it alternates forward and reverse with full lock (`steer ±1`, speed
  −2.7 … +5 m/s), position wandering between x −8.8 and +7.7, y 374 and 389. A three-point
  turn that never completes.
- The two civilians directly ahead of the racers (agents 2 and 34, physical, at y 369 in both
  southbound lanes) stop with `yielding: true` to its siren for the whole time.
- The racers queue behind them, limited by `Obstacle(civilian)`, 40 m short of the finish.
- At about 125 s the unit gets out, the civilians move, three racers finish within 2 s, and
  the fourth is left `Running` at gate 3. That last part is the test failure.

`wp_wasm` `race_tests::short_sprint_with_ai_…` (all-autopilot session of the same race) shows
the same hold at y ≈ 345–360 from about 60 s to its 120 s limit, so it panics "race not over".

**For E (`wp_police`):**
1. A unit's turn-round (round 13: "stop and three-point, or carry on to a junction") must
   finish or give up. This one alternated for 85 s. A cap of a few tries, then carry on to the
   next junction or re-route, would do.
2. Why does an idle or responding unit run its siren here with no pursuit? Most likely a
   reckless-driving response to the race (round-12b dispatch). Civilians yield to any siren
   (D's `siren` module), so a stuck unit with its siren on freezes traffic around it.

I didn't change the race tests for this. Winner 15–60 s and ≥ 3 finishers is still the right
expectation once the unit stops looping.

## Needs
- E: the turn-round loop and siren in section 4 (grid_highway_sprint, wasm short_sprint).
- D: `city_siren_traffic_no_siren` civilian collision (red on lead/round12).
- D, N, E: the lantern deadlock items in section 1 (currently masked in seed 31).
- lead/opponent-pace: loop9 racer 2 LeftCorridor DQ at gate 34.
- N: `wp_opponent` `wanted_ai_racer_flees_police_through_the_city` (see Gate).

## Contract change requests
None.

## Changes outside my crates
- `crates/wp_race/tests/race_picker.rs` (L's test): `run_race_with`, `loop9_circuit` split
  into a police-free pace test plus a police-design test.
- `crates/wp_wasm/src/crash_replay_tests.rs` (test only): park the nearest AI racer.
- `apps/simcli/golden/*`: `[bless]` e612750 and bb87490 (lead-owned goldens).
- `crates/wp_sim/src/lib.rs` (lead): unused test variable `b` → `_b` (clippy `-D warnings`).

## Known gaps
- `grid_highway_sprint` (+ `_is_deterministic`) and `wp_wasm` `short_sprint_with_ai_…` stay
  red until E's fix (section 4).
- `city_siren_traffic_no_siren` scenario stays red (D).
- `loop9_circuit` accepts `FinishWindow` for racers the police held up. It does not tell that
  apart from a slow AI; the police-free pace test covers that.

## Gate
Run on the merged tip (lead/round12 @ ecf4d7b + this branch), with `CARGO_INCREMENTAL=0
CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0`:
- `cargo fmt --check`: clean.
- `cargo clippy --workspace --all-targets -- -D warnings`: clean after one fix to lead/round12
  itself. `wp_sim` lib test `ghost_passes_through_cars_but_not_the_world` had an unused `b`
  (from 8a6997a); renamed `_b`, test still passes.
- `cargo build --target wasm32-unknown-unknown -p wp_wasm`: OK.
- `simcli run apps/simcli/scenarios` (release): 85 of 86 pass; red: `city_siren_traffic_no_siren`
  (D, red on lead/round12). `simcli goldens`: all match except that unblessed scenario.
- `cargo test --workspace --no-fail-fast` (debug): 146 test binaries OK, 4 red. None of them
  is caused by this branch, which changes no sim code:
  - `simcli` `scenarios.rs`: `all_scenarios_pass_and_are_deterministic_in_process` and
    `goldens_match`, both only `city_siren_traffic_no_siren` (D).
  - `wp_race` `race_picker::grid_highway_sprint` (E, section 4). `_is_deterministic` passes.
  - `wp_wasm` `race_tests::short_sprint_with_ai_runs_to_results_and_pays_through_the_economy`
    (E, section 4).
  - `wp_opponent` `tests/race.rs::wanted_ai_racer_flees_police_through_the_city`
    (race.rs:331). The flagged racer flees as intended, but a *second* AI racer also ends
    `Fled`, and the test allows only `Racing` or `Busted` for the others. It's the sim's
    own round-13 police making another racer wanted. That's by design (captures and flights
    are allowed), so the test probably should accept `Fled` for the others. Left to N, since
    it is outside this brief and N owns the flee rules.
- My tests: `loop9_circuit`, `loop9_circuit_pace_without_police`,
  `lantern_checkpoint_draws_the_police`, `hollins_hill_sprint`, `wp_wasm` crash replay
  (4 of 4), `simcli_golden_replays_play_back_in_the_browser_core`: green.
