# Brief: vehicle physics lead (dedicated instance)

**Owner request (2026-10-22):** "make a spin-off instance to only address vehicle physics, while the
other stuff builds out — that should make physics development much faster."

**Repo:** `IllumaestheticJ/WreckPursuit`. Your branch: **`lead/physics`**, based on `lead/round12`.
**Do not push to `lead/round12` or `main`.** The integration lead (session
`session_01NucChLBjseRhRB95rvEEiV`) merges `lead/physics` into round12 when you say it is ready.

**Read first:** `CLAUDE.md`, `docs/INTEGRATION.md`, `docs/CONTRACT_CHANGELOG.md`, then
`docs/reference/wf-clips/` (all `.md` files), `docs/PLAYTEST_LOG.md` rows 101–147 (handling rows),
and `docs/integration/grip-balance.md`.

## Your scope
You own the vehicle physics and how it feels to drive.
- **Code:** `crates/wp_vehicle` (chassis, suspension, tyres, drivetrain, steering, assists).
- **Data:** `data/cars/*.json` handling fields, `data/tires/*`.
- **Lab:** the hidden tune panel `apps/testbed-web/src/drift_lab_w.ts` (`?tune=1` / F10).
- **Tests:** the handling benches and examples.
- **Out of scope:** police, traffic, AI racing logic, world, audio, UI, and damage tables.
  Leave AI pace to `wp_opponent` / `wp_race`, but you must **not** make AI racers slower or crash
  more. Check that with the pace sweep below.

The rest of the team keeps building the other parts in parallel. The owner talks to you directly
about handling, so you don't wait on the lead for every iteration.

## The target: the reference game's keyboard handling
The owner's goal is "Wreckfest-like" sim-cade handling on a keyboard. The reference clips are in
`docs/reference/wf-clips/`:
- the clips themselves, with the keyboard overlay;
- the key timelines (`*_track.json`, `*_keys.json`);
- the yaw extracted from the cockpit (`*_yaw.json`);
- `OWNER_KEY_TECHNIQUE.md`, the owner's tap/hold statistics.

The tools are in `tools/ref-clips/` (`yaw_from_clip.py`, `held_keys.py`, `run_all.sh`). The
reference cars use role ids `parity_rwd_e` (5-speed RWD midsize) and `parity_rwd_f` (6-speed RWD
wagon). **Never write the reference game's car names (CLAUDE.md rule 9).**

What the clips show:
- **Short taps** (0.1–0.6 s) most of the time, **long holds** (3–8 s) through long drifts, throttle
  held most of the time, and the handbrake rarely.
- **A held key steers exactly where pressed** and takes the wheel past 90° even mid-drift.
  **A held key reaches full lock at any speed** (row 147).
- **Hands-off (no key) the car countersteers by itself, capped at about 30 % of lock.** The wheel
  marker settles at p50 16–28°, max 34–47°.
- **Held countersteer plus throttle holds a drift for 3–8 s.** Ours holds **2.24 s** before it swaps
  or spins. That gap is the main open problem.
- **TCS Half is "far more controllable"** (the owner's words) than Off with the same technique.

## Owner rules (never break these)
1. **One physics for every car and both damage modes.** Player, AI, police and traffic all use the
   same vehicle physics. Nothing outside `wp_damage` tables may branch on `DamageMode`.
2. **Key = key.** No auto-steer, no transition flicks, no full-opposite-lock transition aid, no
   "correction algorithm" (rows 137, 138, 141, 142, 143). The only steering help is the hands-off
   countersteer, capped at about 30 %, with a player slider defaulting to 30 %.
3. **Full lock on a held key at any speed** (row 147). Do not shrink the player's lock with speed,
   and do not cut lock for every car, to make drifts hold.
4. **TCS Off = the throttle you press.** TCS and ABS levels are Off / Half / Full (rows 104, 105).
5. Assists (`AssistProfile`) are independent of damage mode.
6. Determinism: no wall-clock time, a seeded `wp_math::Rng`, f32, no `HashMap` iteration order in
   the sim.
7. No licensed or real car names anywhere.

## Where it stands (2026-10-22)
Already shipped on round12:
- key = key steering and the 30 % hands-off cap (4c9d059b);
- countersteer help 0 % by default (0e9bc478);
- the owner's soft rear ×0.45 and firm ride (rows 120, 126);
- front brake bias +0.05;
- punctured tyres run on the rim (row 132).

The held-key drift search was run by session `session_01FKfdoiNS5WT1M85cD1WAu9` (grip-balance) on
branch `lead/grip-balance`:
- **Tool:** `crates/wp_vehicle/examples/held_search.rs`. Seeded; scores held-countersteer drift
  time. Its guard rails are 36 tap runs, `handling_balance` on 17 cars, and AI pace.
- **Candidates:**
  - B: car-wide lock ×0.75. It broke the AI on the Hollins hairpins.
  - D: a player-only speed-sensitive rack at 49 km/h. It held 3.92 s in the bench, but **the owner
    rejected it**: "works very poorly", "doesn't let me steer to full lock".
  - E: a smaller player rack change. It held 3.09 s.
- **Lesson:** the bench rewarded cutting lock, which the reference does not do. Every candidate's
  common recipe was tyre stiffness back toward the car data (undoing the soft rear) plus more yaw
  inertia.
- **Next step already requested** (grip-balance may be mid-way; **you take this over**): re-search
  with lock fixed. Search only tyres, spin model, relaxation, grip balance, yaw inertia and ride.
  Add a guard rail that a held key reaches ≥ 90 % of today's steer angle at 60 / 100 / 140 km/h.
  If no lock-preserving setting holds meaningfully longer than 2.24 s, look for what else explains
  the gap. One example is how the reference may limit the front tyres' slip angle, not the wheel
  angle.

Other useful tools:
- `crates/wp_vehicle/examples/owner_replay.rs` and `row141.rs`: replay the owner's clip inputs.
- `held_counter_trace.rs` and `entry_hold.rs`: drift traces.
- `brake_stability.rs`.
- `crates/wp_race/tests/opponent_pace.rs` with `WP_PACE_TYRE` / `_GRIP` / `_RIDE`: the AI pace
  sweep with lab overrides.

## How to work
1. **Fast loop with the owner.** Build your own **physics preview** artifact (a separate URL from
   the lead's preview) and give the owner the link:
   - build with `OUT=<your scratchpad> tools/preview_build.sh <version>`;
   - smoke-test it (load the page, drive the scripted city chase, no page errors);
   - publish with the Artifact tool.

   Put candidates in the hidden tune panel as clearly labelled presets, so the owner can A/B them
   live without a rebuild. Ask the owner short, concrete questions: "does it spin, snap back, or
   not start?". Ask for keyboard-overlay clips when feel is unclear, and analyse them the way
   `docs/reference/wf-clips/*.md` does.
2. **Log every owner remark** as a new row in `docs/PLAYTEST_LOG.md`. Continue from the highest
   row number, and use `physics-` prefixes if rows collide with the lead's.
3. **Only change the shipped defaults when the owner picks a candidate.** Then, on `lead/physics`:
   - make it the default;
   - re-range any test that encodes the old handling, with a reason;
   - re-bless the goldens it moves (`cargo run --release -p simcli -- goldens --bless`), with the
     reason in the commit;
   - re-run the AI pace sweep and the scenario expectations;
   - tell the lead it's ready to merge, with the hash, what moved, and why.
4. **Gate before each push** (env `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0
   CARGO_PROFILE_TEST_DEBUG=0`):
   - `cargo fmt`;
   - `cargo clippy --workspace --all-targets -- -D warnings`;
   - `cargo test -p wp_vehicle -p wp_race -p wp_opponent`;
   - `cargo build --target wasm32-unknown-unknown -p wp_wasm`;
   - `cd apps/testbed-web && npm run wasm && npx tsc --noEmit -p .`;
   - `node e2e/drift_lab_w.mjs`.
5. **Keep up with round12.** Merge `origin/lead/round12` into `lead/physics` regularly (merge
   commits, no rebase), so the lead's merges stay small.
6. **Performance budget:** vehicle sim ≤ 0.15 ms per car per frame, native release. Note
   measurements in `crates/wp_vehicle/README.md`.
7. Record your work in `docs/integration/physics.md`: decisions, candidates, numbers, and open
   questions.
