# Assist levels: TCS / ABS Off / Half / Full (lead helper `lead/assist-levels`, 2026-10-20)

Owner request (direct, 2026-10-20), playtest rows 104 / 105, reference clips 4 / 5
(`docs/reference/wreckfest-keyboard.md`). Built on `lead/round12`.

## What changed

**Contract (lead-approved amendment, `docs/CONTRACT_CHANGELOG.md`).** `AssistProfile::tcs` is a
`TcsLevel`, `AssistProfile::abs` an `AbsLevel`, each `Off | Half | Full`. A level is written as the
string `"Off"` / `"Half"` / `"Full"` in JSON and RON; the legacy bool still loads (`true` → `Full`,
`false` → `Off`). In RON scenarios write `tcs: "Half"` (quoted): a bare `Half` identifier is not
accepted, because RON's self-describing path drops the variant name. `replay.bin` is unchanged
in layout: the assists op's flag byte gains bit 32 (ABS Half) and 64 (TCS Half).

**TCS Off = the throttle you press (row 104).** With TCS Off:
- no pedal filter (`assists::throttle_filter_strength` returns 0, whatever `throttle_filter` says);
- the slide-throttle feathering in `slide.rs` is **deleted** (it was TCS-off only);
- the drift-hold throttle trim is off;
- the testbed keyboard throttle key has no ramp (`input.ts` `setRawThrottle`; the parity
  harness mirrors it).

The drift lab's "throttle smoothing" slider (`throttle.rs`) is untouched: lab only, default 0.

**TCS levels (`assists.rs`).**
| level | throttle cut | brake-TC |
|---|---|---|
| Off | none | none |
| Half | integral hold of the driven slip at `TCS_HALF_TARGET` = 3.6 × the tyre's peak (3 × Full's 1.2 × start), gain 0.8 /s per × peak (error clamped ±2), plus 0.08 per × peak above the target for that tick; max cut 0.9 | as Full |
| Full | unchanged: proportional from 1.2 × peak to 0.9 cut at 3.2 × peak | unchanged |

Why Half is integral: a slip target past the tyre's peak is an unstable balance. A proportional
cut there was bistable: it either lost the wheels (Half ≈ Off: mean slip 2.0 vs Off's 2.4 in
`rampart_50`) or snapped them back to grip (Half ≈ Full). An unclamped integral limit-cycled at
~5 Hz between grip and 0.9 slip; the clamped PI above holds ~0.30 slip steadily through 1st.

**ABS levels (`brakes.rs`).** Full releases a wheel past 1.4 × peak slip (unchanged). Half
releases past 3 × peak (capped at 0.95, so a locked wheel is always released): deeper slip and
short lock-ups, still steerable. Off: no modulation (and any pending release is cleared).

**Presets (`presets.rs`, ui-kit `ASSIST_PRESETS`).** Rookie = TCS Full / ABS Full; Street = TCS
Half / ABS Full (a little wheelspin to slide with); Sim = TCS Off / ABS Half (the owner's own
reference setup, clip 5). Device defaults (`AssistProfile::keyboard_default` etc.) keep their
old meaning (`true` → Full), so no AI / traffic / police behaviour changed. Drift-lab profiles
were already TCS Off and stay so. Touch default TCS Full (was `true`). Game-menu default ABS
Full, TCS Off (was ABS on, TCS off).

**UI.** Testbed assist row: ABS / TCS selects (same style as the clutch select). Game menu (also
the touch settings, which use the same menu): `Cycle` rows Off / Half / Full with per-level
hints. ui-kit assist matrix: level selects. HUDs: `TCS` at Full, `TCS HALF`, `TCS OFF` (same for
ABS).

## Measurements

Parity harness (`just parity`; owner keyboard settings, manual box with redline upshifts,
4 s from rest, throttle key held, no steering). Full table in
`docs/reference/wreckfest-parity.md`.

| car | TCS | time at slip > 0.2, s | mean driven slip | peak heading change ° |
|---|---|---|---|---|
| rampart_50 | Off | 3.82 | 2.42 | 0.0 |
| rampart_50 | Half | 0.61 | 0.13 | 0.0 |
| rampart_50 | Full | 0.18 | 0.06 | 0.0 |
| shiro_turbo | Off | 3.17 | 1.21 | 0.0 |
| shiro_turbo | Half | 0.82 | 0.14 | 0.0 |
| shiro_turbo | Full | 0.25 | 0.08 | 0.0 |
| kestrel_ts | Off | 3.19 | 1.51 | 0.2 |
| kestrel_ts | Half | 0.70 | 0.13 | 0.4 |
| kestrel_ts | Full | 0.12 | 0.05 | 0.4 |
| parity_rwd_a (reference spec) | Half / Full | 1.38 / 0.20 | 0.24 / 0.06 | 0.0 |

Half's mean slip is 2–4 × Full's. In the parity run the redline upshift ends Half's spin early
(spinning wheels reach the redline sooner). Held in 1st (simcli `assists/launch_tcs_half`), Half
spins the whole 4 s at ~0.3 slip to 17.7 m/s (40 mph), against Full's 19.9 m/s and Off's
13.1 m/s (Off spins 3.8 s at slip 2–6).

**Launch straightness (row 105 target: yaw < 3° with TCS Off).** Met on every car and level:
≤ 0.4° on the flat pad. No physics cause to report from this harness. If the owner sees the car
yaw on a TCS-Off launch in the browser, it is not reproduced on a flat pad with no steering. The
next suspects are camber / road crown on city roads, or a steering input from the key layer,
not the TCS levels.

60–0 mph on the brake key:

| car | ABS Off | ABS Half | ABS Full |
|---|---|---|---|
| rampart_50 | 3.55 s / 47.9 m | 2.88 s / 39.2 m | 2.80 s / 37.6 m |
| shiro_turbo | 3.45 s / 47.1 m | 2.73 s / 37.2 m | 2.75 s / 36.8 m |
| kestrel_ts | 3.58 s / 48.8 m | 2.85 s / 38.7 m | 2.89 s / 38.4 m |

Braking from 80 mph with a 0.2 s steering tap (clip 5): peak body slip 2.5–3.7° with Half
(4.2–5.7° Full, 0.4–0.7° Off: locked fronts don't steer). Half still steers; nothing spun. Half
is only ~0.5–1.6 m longer than Full: that matches "short lock-ups, still steerable". If the
owner wants Half to feel more like lock-ups, raise `ABS_HALF_TRIGGER` (6 × peak gave ≈ Off).

## Tests

- `wp_contracts`: legacy bool / string / junk in JSON and RON; names round-trip.
- `wp_vehicle`: per-level TCS cut (`tcs_levels_order_their_slip_targets`), Half holds a spinning
  wheel near its target (toy plant), raw throttle filter with TCS Off, per-level ABS release, ABS
  braking per level on `debug_sedan` (`abs_prevents_lockup_and_shortens_stops`: Half between
  Full and Off), presets (incl. Sim's keyboard filter is inert).
- simcli: `replay.bin` Half bits round-trip, pre-level flags read as Full / Off. Scenarios
  `scenarios/assists/launch_tcs_{off,half,full}.ron` and `brake_abs_{off,half,full}.ron`, with
  new metrics `wheelspin_s` and `max_heading_change_deg`.
- ui-kit vitest: levels, legacy bools, junk. testbed e2e: presets (levels, HUD tags, legacy
  overrides), menu (cycles reach the sim + assist row), touch, countersteer, controls, gamepad,
  drift lab: all pass.

## Goldens ([bless])

Only two existing goldens moved: `parity_rampart_launch_tcs_off` and
`parity_rampart_counter_drift`. Both run TCS Off with `throttle_filter` 0.7, so their keyboard
throttle is now raw. Their expectations still pass; counter-drift's pending "no spin" gap is
unchanged. Six new goldens are for the assist scenarios. `golden_fwd_liftoff.replay.bin` changed
only in its embedded scenario text (`"Off"` instead of `false`); its hashes are identical.

## Gate

fmt ✓ · clippy `-D warnings` ✓ · `cargo test --workspace` ✓ except
`wp_opponent::race::wanted_ai_racer_flees_police_through_the_city`, which **fails identically
on `lead/round12`** (pre-existing: "drove only 167 m after the call") · wasm build ✓ ·
`lint-determinism` 0 errors · ui-kit and testbed `tsc` ✓ · e2e presets, menu, touch,
countersteer, controls, gamepad, drift lab ✓. The menu e2e's new level clicks park the mouse
afterwards: a pointer left over the menu stole the focus of later checks. In the full `run.mjs`
the perf checks fail on this sandbox's software GL (~4 fps; environment, not code). The reset
check failed once under that load and passes alone, on this branch and on `lead/round12`.

## Found, not fixed (outside this task)

- **Automatic gearbox on wheelspin (TCS Off).** With `auto_gears`, a floored TCS-Off launch in
  `rampart_50` upshifts 1 → 2 at ~6 mph, because the spinning wheels put the engine at the
  redline. It then hunts 1 ↔ 2 up to ~30 mph. With the manual box (the owner's setting) the
  car stays in 1st as the reference does. The auto-shift logic should judge by ground speed
  while the driven wheels spin. For A (vehicle, `drivetrain.rs` `auto_shift`).

## Contract change requests

None beyond the approved amendment.
