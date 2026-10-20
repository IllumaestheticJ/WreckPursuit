# Drift Lab B — keyboard countersteer + semi-intelligent drift assist

Branch `lead/drift-lab-b` (from `lead/round12`), owner rows 90 / 91 (PLAYTEST_LOG). This is the
"B" side of a deliberate A/B: machine A builds its own version on `ws/vehicle`; the owner
compares both and picks one.

**Where:** `crates/wp_vehicle/src/drift.rs` (tuning, state, modes), `steering.rs`
(`slide_onset`, `countersteer_rate`), `drift_lab.rs` (scripted measurements),
`examples/drift_lab_b.rs` (the tables below), `crates/wp_wasm/src/drift_lab.rs` (tune panel
wiring), `apps/testbed-web/src/drift_lab_b.ts` (course + panel), one menu entry in
`apps/ui-kit/src/menu/GameMenu.tsx`.

**How to try it:** testbed → *drift lab B* button (developer panel), or the start menu's
*Drift Lab B*, or `?drift=b&car=shiro_turbo`. The **tune** button (top right) opens the sliders;
changes apply at once as a runtime override (remembered in this browser only, *defaults*
resets, *copy JSON* copies the values). The readout shows slide angle, assist mode, envelope,
authority, throttle shaping and onset. Rules kept: same code path in Arcade and Simulation,
deterministic (f32, no wall clock), no yaw torque, no grip added; the wheel preset
(`countersteer_help` 0, no speed sensitivity) is untouched.

Re-run every table: `cargo run --release -p wp_vehicle --example drift_lab_b` (`catch` or
`drift` for one half). Keyboard preset with TCS/ESC off unless noted.

## Changelog

### Step 1 — countersteer response speed (row 90)

**Measured root cause.** The keyboard rack rate was `lock / (STEER_RAMP_S × sensitivity)`
capped at the car's rack speed. In a slide the countersteer lock opened, so the cap — the car's
rack speed, 1.0–1.7 rad/s — was the limit: **~80–117 °/s at the wheels at 60–140 km/h**, i.e.
~0.4 s to full lock, plus the countersteer lock and the self-aligning target only opened once
rear-axle slip passed 0.06 rad (lock) / 0.03 rad (alignment).

**Changed.**
- `steering::slide_onset`: one 0..1 onset from rear-axle slip (0.025 → 0.06 rad) **or** its
  growth rate (0.3 → 0.6 rad/s, from half the onset angle) — a slide is met while it starts,
  not once it has built. The countersteer lock, the self-aligning target and the fast rate all
  use it.
- `steering::countersteer_rate`: while sliding, any rack movement toward the countersteer side
  (the player's counter key) or toward the self-aligning target (released keys) runs at an
  absolute rate: **full lock in 0.10 s at the default 60 %** (0.06 s at 100 %, scales with the
  slider; 0 % = the plain model). Turn-in keeps the slow speed-sensitive ramp.
- `DriftState` (rear-slip rate filter, 20 ms) in `Vehicle` and in the snapshot (format 8).
- Telemetry `Telemetry::drift` (mode, slide angle, onset, envelope, authority, throttle).

**Measured (yaw kick 0.8 / 1.2 / 1.6 / 2.0 / −1.2 / −2.0 rad/s, throttle 30 %).**

| car | km/h | countersteer slew °/s (before → after) | released keys caught | 150 ms-late counter key caught |
|---|---|---|---|---|
| shiro_turbo | 60 | 111 → **398** | 6/6 → 6/6 | 6/6 → **6/6** |
| shiro_turbo | 100 | 101 → **398** | 6/6 → 6/6 | 4/6 → **6/6** |
| shiro_turbo | 140 | 99 → **398** | 4/6 → 4/6 | 3/6 → **6/6** |
| sperber_32 | 60 | 117 → **381** | 6/6 → 6/6 | 6/6 → **6/6** |
| sperber_32 | 100 | 100 → **381** | 4/6 → 4/6 | 4/6 → **6/6** |
| sperber_32 | 140 | 98 → **381** | 4/6 → 4/6 | 3/6 → **6/6** |
| rampart_50 | 60 | 112 → **379** | 6/6 → 6/6 | 6/6 → **6/6** |
| rampart_50 | 100 | 102 → **379** | 4/6 → 4/6 | 4/6 → **6/6** |
| rampart_50 | 140 | 97 → **379** | 4/6 → 4/6 | 3/6 → **6/6** |
| debug_sedan | 60 | 105 → **355** | 6/6 → 6/6 | 4/6 → **6/6** |
| debug_sedan | 100 | 98 → **355** | 6/6 → 6/6 | 3/6 → **4/6** |
| debug_sedan | 140 | 98 → **355** | 4/6 → 4/6 | 3/6 → **4/6** |
| junkyard_beater | 60 | 104 → **363** | 6/6 → 6/6 | 6/6 → **6/6** |
| junkyard_beater | 100 | 100 → **363** | 6/6 → 6/6 | 4/6 → **6/6** |
| junkyard_beater | 140 | 98 → **363** | 4/6 → 4/6 | 3/6 → **4/6** |

Totals: late counter key caught **62 → 84 of 90**; released keys 76 → 76 of 90 (the released
driver's wheels now reach the align target within ~25 ms, but at 60 % the target is 0.84 × the
travel and the front tyres then give no restoring force — step 3's *straighten* adds that).
`debug_sedan` / `junkyard_beater` still lose some 100–140 km/h kicks with a late key: they
have the least front grip of the five.

**Owner, try:** Drift Lab B, a RWD car (shiro_turbo, rampart_50), countersteer 60 %. Kick the
rear out on the long straight (handbrake tap or lift mid-sweeper) and hit the counter key late —
the wheels should snap over instead of winding. Slider **slew** in the tune panel changes the
full-lock time if it feels too sharp/soft.

### Step 2 — Drift Lab B (testbed)

- Scenario `drift_lab_b`: the flat world with no traffic, parked cars or police, noon with a
  slow clock. Course (paint and cones only, no physics): a clockwise loop from the spawn —
  160 m straight, 90° R22, 130 m, 90° R22, 160 m, **long sweeper 90° R70**, 82 m, 90° R22,
  48 m — on a 420 m asphalt pad.
- Tune panel: countersteer %, onset, onset full, onset rate, slew (full-lock time at 60 %),
  plus the step-3 constants as they land. Override via `wp_wasm::drift_tuning_set`
  (process-wide, not logged in recordings — a developer tool).

## Known issues seen on the base branch (not from this work)

`cargo test -p wp_vehicle` on untouched `origin/lead/round12` already fails
`differentials::roster_diff_hashes_are_pinned` and
`handling_balance::front_drive_cars_understeer_mildly`; left alone (not re-blessed here).

## Contract change requests

None so far. (`AssistProfile` has no device field; the assist infers keyboard / gamepad /
wheel from `steer_speed_sensitivity`. A `device` field would make that explicit.)
