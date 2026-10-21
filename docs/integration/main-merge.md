# Release merge: `main` into the round-12 line (2026-10-21)

Branch `claude/serene-bohr-cey1t7` = `lead/round12` @ 4ab9703d + `origin/main` @ 3dc962bc
(PR #18 police pro driving, PR #21 car stat bars, "leaving the route is never a DQ") + the
smaller fixes below. This is the candidate for the release to `main`.

## Merge decisions (for the owners of the code involved)

- **Police driving (E / lead P8), `wp_police::PoliceSystem::unit_input`.** Round 12 grew the
  function into a manoeuvre stack (turn round, reverse swing, back out, give way, push-through,
  stuck detection, head-on hold, quiet siren); main replaced its steady step with the pro driver.
  Merged: the manoeuvre stack runs first for every unit; the steady step is the **pro driver
  only while chasing** (`Assignment::Pursue` / `Intercept`, new `pro_input`) and the round-12
  lane driver otherwise (patrol, responses). Why: with the pro driver on every lights-on unit,
  a responder cut the Hollins switchbacks and stopped in the racers' lane (winner 116.8 s,
  field held 30 s); with it on chases only the race is 73.0 s, 4/4, no contacts, and main's
  `police_pace` tests still pass. `pro_input` caps the target at the wall feeler limit and a
  wall at the side pins the line to the shifted lane offset (row 66). It does **not** write
  `Unit::planned_speed` (the think-rate plan the push-through / stuck checks read).
  E: if responses should use the pro driver too, the responder needs to arrive in its own lane
  on narrow roads first (reproduce: `cargo test -p wp_race --test race_picker hollins_hill_sprint`).
- **Opponent AI (N), `wp_opponent::field`.** A car pulling over or out of the race rolls on at
  `PULL_OVER_MPS` looking for a spot for at most `PULL_OVER_MAX_S` (15 s), then stops. Before,
  the roll-on had no limit. `live_police_chase_ends_in_escape_or_capture_without_stuck_cars`
  allows the roll-on for a car booked less than that before the run ends (the faster police now
  catch racer 3 seven seconds before the end).
- **Race rules (L).** Off-route DQ removed (owner call, main). The AI's corridor limit keeps the
  old 2 m margin as `wp_race::entrants::AI_CORRIDOR_MARGIN_M`. `race_sprint_off_route`'s
  open-loop steering was re-timed for the round-12 steering response (it overshot to 35 m and
  left the course); its last step's end (12.2 s) is sensitive to ±0.2 s.
- **Judge police list.** `RaceJudge::add_police` (ids accumulate for the race) replaces the
  per-tick `set_police` in `LiveRace` and the race tests: a retired unit is despawned at the end
  of the tick its last contact is reported, so a bump with it counted as a civilian hit.
- **Net.** The server keeps the typed events a disconnected player would have seen (up to
  `MAX_MISSED_EVENTS` = 512, no police radio) and sends them after the welcome on reconnect.
- Manifests, testbed `main.ts` / `index.html` (stat bars + tune row), READMEs and docs: both
  sides kept.

## Goldens and generated data
`[bless]` commit b6075178: police scenarios with chasing units, `golden_city_chase` (+ replay),
`weather_fog_pursuit(+_clear)`, `race_sprint_off_route`; `data/stats/cars.json` re-measured on
the round-12 vehicle.

## Gate (cloud container, 2026-10-21)
`cargo fmt --check`, `cargo clippy --workspace --all-targets -D warnings`, wasm build,
`tsc` (testbed + ui-kit): pass. `cargo test --workspace`: **161 / 161 suites pass**.
`tools/wasm-determinism/compare.sh`: **native == wasm on all 109 scenarios**.
`PERF=0 node e2e/run.mjs` (release wasm, headless Chromium): **187 pass, 0 fail**.

## Known gaps
- Browser perf check: steady allocation ~460–510 KB/frame (gate 200) and phone draw calls
  313 (gate 300) on software GL; see `browser-perf.md` "Allocation pass (2026-10-20)".
