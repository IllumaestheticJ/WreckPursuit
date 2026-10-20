You are working on WreckPursuit (repo IllumaestheticJ/WreckPursuit), a physics street-racing game: Rust workspace, deterministic 240 Hz sim, browser testbed in apps/testbed-web (Vite + Three.js + wasm).

TASK: make keyboard countersteering catch slides, then make it a semi-intelligent drift assist. This is the owner's top priority.

Setup
- git fetch origin; create branch `lead/drift-lab-b` from `origin/lead/round12` (NOT main). Push only to that branch. Do not create pull requests.
- Read CLAUDE.md (rules: physics identical in Arcade and Simulation; determinism; no wall-clock in the sim), docs/INTEGRATION.md, docs/integration/vehicle.md (rows 56/57/72: Wreckfest-style self-aligning countersteer, no yaw torque), docs/PLAYTEST_LOG.md rows 56, 57, 72, 81, 90, 91.
- Code: crates/wp_vehicle/src/steering.rs (slip_target_lock, steer_target, countersteer_lock, ALIGN_*), assists.rs, slide.rs, vehicle.rs; testbed apps/testbed-web/src/main.ts (countersteer slider `#cstr`, presets).
- Another session (machine A, branch ws/vehicle) is doing the same work in parallel. This is a deliberate A/B: build your own version on your own branch. Do not push to ws/vehicle. The owner will compare both in a "Drift Lab" and pick one.

Owner feedback (keyboard)
1. "The countersteering backwards initiates too slow, making it hard to catch slides."
2. "I believe the root cause is the countersteering happens too slow on the keyboard, so it can't catch the slide."
3. "We want the keyboard countersteering to be semi-intelligent with steering and throttle input, so that there is still punishment for mistakes, but it rewards trying to hold the slide."

Likely root cause (verify it by measuring first)
In slip_target_lock the rack rate is `lock / (STEER_RAMP_S * sensitivity)`, where `lock` is the speed-sensitive slip-targeted lock. At speed that lock shrinks to about the tyre peak slip angle (~0.1 rad), so the rack's absolute slew (rad/s) is small exactly when a catch needs 15–30° fast. Also check that the countersteer lock opens immediately on the counter key, and that the align target isn't gated until the slip has built past ALIGN_SLIDE_START.

Step 1 — response speed (push this first, within ~45 min)
- When moving in the countersteer direction (target sign opposite to the slide, from either the player's key or the align target), use an absolute fast rate based on the car's full lock / rack speed: full lock in ≤ 0.12 s at the default 60%. Open the lock to the slip-needed angle immediately. Keep the slow speed-sensitive ramp only for turn-in at speed (no twitchiness).
- Start aligning earlier: smaller rear-slip onset (e.g. 0.015→0.05 rad) and/or on rising yaw-rate error (yaw rate − path rate).
- Measure before/after: countersteer slew at the wheels (°/s) at 60/100/140 km/h; time from slide onset to wheels aligned with the travel (error < 3°); catches vs spins for a released-keys driver and for a driver who counter-steers 150 ms late. Cars: shiro_turbo, sperber_32, rampart_50, debug_sedan, junkyard_beater.

Step 2 — Drift Lab (in the same first push)
- A testbed start-menu / scenario entry "drift lab B": a big open asphalt pad, a few 90° corners and one long sweeper. No traffic or police, daytime.
- A live tuning panel (button "tune") with sliders for your main constants (onset, slew, hold gain, throttle shaping, envelope angle, countersteer %), written to a runtime override (not the data files). A readout of slide angle, assist mode and envelope.

Step 3 — semi-intelligent assist (keyboard first, a lighter version on gamepad, the wheel untouched)
- Intent modes, read every step:
  - steering released → straighten (fast align)
  - key into the turn + throttle → hold: target the current body slip angle via the front wheel angle, so a binary key gives a steady drift; taps nudge the angle
  - key against the slide → catch: fast countersteer plus extra lock
- Throttle shaping: map the binary throttle key to a smooth partial throttle that tends toward holding the angle. More throttle still grows the angle; a lift shrinks it.
- Bounded authority (mistakes cost):
  - only helps up to a slide-angle envelope (~35° at 60%, ~45° at 100%); past that, or with yaw acceleration it can't cover, it backs off and the car can spin
  - full throttle at a big angle in a high-power RWD car still spins
  - an abrupt lift still snaps (softened, not cancelled)
  - late or wrong counter-input past the envelope isn't saved
  - never adds grip, never applies a yaw torque: steering angle and throttle shaping only
- Reward: a smooth "hold + modulate" driver holds long, steady drifts through a 90° corner and a sweeper. The envelope widens slightly after ~1 s of a stable hold and shrinks with erratic inputs.
- Expose the assist mode (straighten / hold / catch / exceeded) in telemetry.
- Measure with scripted drivers (good / naive / sloppy / abrupt-lift) at 30/60/100%. Sloppy must spin in a defined, non-zero share of runs.

Working style
- Short cycles. For each working step: `cargo test -p wp_vehicle`, `cd apps/testbed-web && npm run wasm && npx tsc --noEmit -p .`, then push. Use CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 (disk is tight).
- Commit messages prefixed with the crate (e.g. `wp_vehicle: absolute countersteer slew`).
- Keep a short changelog and a measurement table in docs/integration/drift-lab-b.md, saying what changed and what the owner should try in each step.
- Before the final push: cargo fmt, cargo clippy --workspace -- -D warnings, cargo test --workspace, all simcli scenarios. Bless goldens only in a `[bless]` commit with the reason.
- Don't touch data/modes, wp_contracts (propose contract changes in your doc), or crates outside wp_vehicle and the testbed, except minimal wiring in wp_wasm.
