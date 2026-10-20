# Reference-game parity (wf-parity) integration notes

- **Workstream:** lead helper, owner direction 2026-10-20 ("all of our driving physics and similar
  to be exactly like" the owner's reference game, Wreckfest). Measure and report: no tyre or
  steering physics changes here (A and the Drift Lab sessions own them).
- **Branch:** `lead/wf-parity` (from `lead/round12`)
- **Crates / folders touched:** `crates/wp_vehicle/examples/parity.rs` (new example only),
  `apps/simcli/scenarios/parity/`, `docs/reference/wreckfest-parity*.{md,toml}`, `justfile`
  (`just parity`), the controls settings in `apps/ui-kit/src/menu/` and `apps/testbed-web/src/`.
- **Contract version built against:** 0.3.0 (unchanged; no contract edits)

## 1. Parity measurement suite

`just parity` (= `cargo run --release -p wp_vehicle --example parity -- --write`) measures the
owner's keyboard settings on `rampart_50` (heavy RWD muscle, clips 1 and 4), `shiro_turbo` (light
RWD sports, clip 2) and `kestrel_ts` (FWD touring, clip 3). It rewrites the table in
[docs/reference/wreckfest-parity.md](../reference/wreckfest-parity.md). The run takes about 3 s
release on one core of the cloud container (the budget was < 2 min). The page also has the setup
and the **gap list**.

- Reference values: `docs/reference/wreckfest-parity-reference.toml`, one `[[ref]]` per
  (metric id, car). Numbers, ranges, bounds (≥ / ≤), ≈ values, yes/no, or free text. The lead
  adds rows from the owner's clips, then re-runs `just parity`.
- The keyboard driver is a copy of the testbed's input layer (`input.ts`: 60 Hz ramps, steer
  4/s in and out, throttle 12/s, brake 6/s / 10/s), then the controls shaping below. If
  `input.ts` ramps change (e.g. `lead/drift-twitch`'s 6/s), update `Keyboard` in `parity.rs` in
  the same merge.
- Drift Lab C's drivers aren't in `lead/round12`. The countersteer drift row uses the same moves
  as its "Naive" driver. After Drift Lab C merges, switch that row to `drift_lab::DriftDriver`.
- simcli guards: `apps/simcli/scenarios/parity/*.ron`. `expect` checks today's numbers (loose
  ranges); `expect_pending` holds the reference target (reported, never fails). When a gap
  closes, the owning workstream moves the pending range into `expect`.

## 2. Controls settings (per device)

Per device (keyboard / gamepad / wheel): steering dead zone, steering saturation, steering
sensitivity, steering speed sensitivity, and accelerator / brake / clutch dead zone +
saturation. Live test bars show the processed input. They live in the game menu (Settings →
Controls) and in the testbed controls panel. Both edit one stored copy:
`GameSettings.controls`, kept in `localStorage` under `wp.gameSettings.v1` and never shared.
The shaping is applied in the testbed's input layer.

**Pipeline:** keys → testbed ramps (`input.ts`) → shaping → `DriverInput` → `VehicleInput`
(analogue values arrive already shaped, as the contract says) → `wp_vehicle` assists (pedal
filters, steering). Gamepad: stick deadzone + curve (bindings) → shaping. A pad whose id
contains "wheel" uses the wheel profile.

**Shaping** (`apps/ui-kit/src/menu/controls.ts`; mirrored in `parity.rs`; all values are
fractions 0..1 and shown as %):

```text
steer:  a = |x|;  t = a ≤ dz ? 0 : min(1, (a − dz) / (sat − dz));  out = sign(x) · t^(1 + 2·(1 − sens))
pedal:  t = x ≤ dz ? 0 : min(1, (x − dz) / (sat − dz))
```

- **Dead zone** (0–50 %): input below it gives 0, and the rest is rescaled.
- **Saturation** (50–100 %): the input level that already gives full output. 100 % means full
  output only at full input.
- **Sensitivity** (0–100 %): the response curve. 100 % is linear. Lower values soften the centre
  (50 % = a squared curve, 0 % = a cubic one). The reference game's exact curve isn't known. The
  owner's keyboard value (100 %) gives the identity either way, so their setup doesn't depend on
  this guess. Their recordings of the test bars can confirm it later.
- **Steering speed sensitivity** (0–100 %) maps to `AssistProfile::steer_speed_sensitivity =
  % / 100` for the active device, and to 0 when the menu's "Steering help" is off. Our field
  means:
  - 0 = off: full lock at every speed, the car's own rack speed (the wheel preset).
  - Above 0, the lock is slip-targeted: a held key asks for the front tyres' peak slip angle
    plus the cornering angle, so the lock shrinks with speed (less lock at speed, which is what
    the reference setting does).
  - Lower values add margin (`1.1 + 0.4·(1 − s)/s` × peak slip: 75 % → 1.23×, 100 % → 1.10×)
    and shorten the rack time (`0.16 s × s`: 75 % → 0.12 s).

  So "75 %" is a little less speed reduction than 100 %, as in the reference. Both ends match:
  0 % = no speed sensitivity, 100 % = the most. Measured: the owner's 75 % against our old
  keyboard preset (100 %) makes a 0.1 s tap at 100 mph ~10 % stronger (heading 3.6–4.3° vs
  3.3–3.8°). The 60 mph key-held circle barely changes (75.6–83.7 m vs 75.5–83.3 m).
- **Defaults:**

  | device | dead zone | saturation | sensitivity | speed sensitivity | pedals |
  |---|---|---|---|---|---|
  | keyboard | 0 % | 100 % | 100 % | **75 %** | 0 % / 100 % |
  | gamepad | 0 % | 100 % | 100 % | 70 % | 0 % / 100 % |
  | wheel | 0 % | 100 % | 100 % | 0 % | 0 % / 100 % |

  Keyboard = the owner's settings. Gamepad and wheel keep their old presets' values (0.7 and 0),
  and the shaping is the identity, so the only behaviour change is keyboard speed sensitivity
  1.0 → 0.75.

No contract field is needed. Shaping happens before `VehicleInput` (so replays and the net
carry the shaped values), and speed sensitivity reuses `steer_speed_sensitivity`.

## Exposes
- `cargo run --release -p wp_vehicle --example parity [-- --write]` / `just parity`;
  `PARITY_SPEED_SENS=<0..1>` for A/B runs.
- `docs/reference/wreckfest-parity-reference.toml` (format in its header).
- ui-kit: `ControlDevice`, `DeviceControls`, `defaultDeviceControls`, `shapeSteer`, `shapePedal`,
  `GameSettings.controls`, `GameMenuHost.inputProbe`.

## Needs
- **A (`ws/vehicle`):** `TcsLevel` (row 105) so the TCS Half rows can be measured (WP-1).
- **Drift Lab C:** merge into the lead branch, then this suite switches to its drivers (G1, G3).
- **Owner:** recordings of the reference game's test bars (keyboard tap / hold) to pin its key
  ramp rates and sensitivity curve; dedicated clips for braking (G6) and taps at a known speed
  (G4).

## Contract change requests
| # | Change (type / field / variant) | Why | Who is affected | Status |
|---|---|---|---|---|
| WP-1 | `AssistProfile::tcs: bool` → `TcsLevel { Off, Half, Full }` (legacy bool accepted: `false` → Off, `true` → Full). This is the already-approved row 105 change; the request is about naming: **Half / Full** as in the reference game (row 105 says Low / High) | clip 4 is driven with TCS Half. The parity rows `launch_*_half` read "n/a" until it exists | A (implements), wp_wasm, testbed + ui-kit (3-way control), simcli scenario/replay assists, wp_net | requested |

## Changes outside my crates
- `crates/wp_vehicle/examples/parity.rs`: a new example, no library code changed.
- `justfile`: `parity` recipe.

## Perf numbers
| Measurement | Machine | Native | Browser | Budget |
|---|---|---|---|---|
| `just parity` (3 cars, ~80 manoeuvres) | cloud container, 1 core | 2.6 s release | — | < 2 min |

## Known gaps
- TCS Half isn't measurable yet (WP-1).
- The countersteer drift row uses a script equal to Drift Lab C's "Naive" driver, not the lab's
  own code (not merged).
- Reference numbers are few, and some are rough or qualitative (braking is derived from a
  101 → 65 mph stop with downshifts; the tap target "≈ a lane change" is qualitative). The
  table marks every source.
- The sensitivity curve shape (`1 + 2·(1 − sens)`) is our guess. It is the identity at the
  owner's 100 %.
