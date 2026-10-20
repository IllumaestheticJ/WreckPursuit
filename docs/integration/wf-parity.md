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
owner's keyboard settings on eight cars. It rewrites the table in
[docs/reference/wreckfest-parity.md](../reference/wreckfest-parity.md). The run takes about 7 s
release on one core of the cloud container (the budget was < 2 min). The page also has the
setup, the "car data or model?" comparison, the **gap list** and the tune-screen proposal. The
cars:
- the roster analogues `rampart_50` (heavy RWD muscle, clips 1 and 4), `shiro_turbo` (light RWD
  sports, clip 2) and `kestrel_ts` (FWD touring, clip 3);
- four parity-only reference cars built to the owner's specs: `parity_rwd_a/b/d` and
  `parity_fwd_c`. `parity_rwd_d` is the primary drift-hold car;
- `parity_rwd_a_tuned`, which applies the owner's clip-5 tune through the proposed mapping.

- **Parity-only cars:** `crates/wp_vehicle/examples/parity_cars.toml` (a test fixture, not
  `data/cars`). Each one is a roster `base` with the reference mass, torque curve and gearing.
  Inertias, springs, dampers, ARBs and brakes scale with mass. They aren't in the roster, so the
  roster checks, the economy and spawn lists never see them. When the lead adds more specs,
  add a `[[car]]` block.
- **Tune mapping:** `Tune` / `apply_tune` in `parity.rs` is the proposed 5-step mapping (table
  in the parity doc). It is measurement-only until G10 is built.

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

**Keyboard: what is meaningful (lead note, owner test 2026-10-20).** The reference's "Test
steering" bar on the keyboard jumps straight between −100 % and +100 %: its keyboard input is
raw, and all smoothing happens in the car's steering. Drift Lab C is moving our keyboard
smoothing into `wp_vehicle` steering too. So:
- The test bars show the **processed `DriverInput`**, the value sent to the sim. Today, for the
  keyboard, that still includes the testbed's 4/s key ramp, so a held key climbs over 0.25 s.
  Once the ramp is gone, the bars jump −1 / 0 / +1 like the reference's. Nothing in the bars
  needs to change.
- On a binary key, steering **dead zone, saturation and sensitivity are mostly cosmetic**. A
  dead zone below 100 % passes a full key unchanged, and saturation and sensitivity only
  reshape the values in between. Today those are the ramp's in-between values, so for example
  saturation 50 % halves the time to full lock; with raw keys there are none. The same holds
  for the pedal dead zone and saturation on keys. They matter for gamepads and wheels.
- **Steering speed sensitivity (75 %) is the meaningful keyboard setting**. It sets how much
  lock a held key gets at speed, inside `wp_vehicle`.

**Merged UI work** (helper session, commits `4c0e1b53`, `942cedca`, `5b140bfd`): the menu's
Settings → Controls section has a device cycle, sliders, *Reset device*, live bars, and a *Test
input* toggle. While it is on, the menu ignores navigation keys and Esc ends it. The testbed
controls panel has the same sliders and bars, through `menu.update({controls})`. `input.ts` has
the shaping plus `probeInput` (works while game input is blocked). On the debug hook:
`__wp.inputProbe` / `deviceControls()` / `activeDevice()`. The preset highlight
(`activePreset()` in `main.ts`) ignores the speed-sensitivity *value* and compares only on/off,
because it is a per-device setting, not part of a preset. `e2e/gamepad.mjs`'s preset check now
skips that one field for the same reason. e2e: `npm run e2e:controls`.

## Exposes
- `cargo run --release -p wp_vehicle --example parity [-- --write]` / `just parity`;
  `PARITY_SPEED_SENS=<0..1>` for A/B runs.
- `docs/reference/wreckfest-parity-reference.toml` (format in its header).
- ui-kit: `ControlDevice`, `DeviceControls`, `defaultDeviceControls`, `shapeSteer`, `shapePedal`,
  `GameSettings.controls`, `GameMenuHost.inputProbe`.

## Needs
- **A (`ws/vehicle`):** `TcsLevel` (row 105) so the TCS Half rows can be measured (WP-1), ABS
  levels (WP-3), and the handbrake declutch keeping the drive for short stabs with the throttle
  held (G8).
- **Drift Lab C:** merge into the lead branch, then this suite switches to its drivers (G1, G3).
- **Owner:** recordings of the reference game's test bars (keyboard tap / hold) to pin its key
  ramp rates and sensitivity curve; dedicated clips for braking (G6) and taps at a known speed
  (G4).

## Contract change requests
| # | Change (type / field / variant) | Why | Who is affected | Status |
|---|---|---|---|---|
| WP-2 | `AssistProfile::keyboard_default().steer_speed_sensitivity` 1.0 → **0.75** (the owner's value) | the testbed now sends 0.75 for the keyboard. simcli `assists: keyboard`, replays and other hosts still get 1.0 from the contract default, so the two differ | wp_contracts, simcli keyboard scenarios (goldens using `keyboard`), wp_vehicle presets tests | requested (optional; the testbed doesn't need it) |
| WP-3 | `AssistProfile::abs: bool` → `AbsLevel { Off, Half, Full }` (legacy bool: `false` → Off, `true` → Full) | run 5 / clip 5 use ABS Half; G7 | A (implements: Half = a looser slip target / slower release cycle), wp_wasm, testbed + ui-kit (3-way control), simcli assists, wp_net | requested |
| WP-4 | `CarTune { suspension, gearing, diff, brake_balance: u8 }` (1..=5, 3 = stock) + an optional per-entity spawn field | tune screen G10: server, net spawn and replays must apply the same tune | wp_sim (spawn), wp_net, simcli (entity field), wp_economy (garage state), ui-kit | proposal (design in the parity doc) |
| WP-1 | `AssistProfile::tcs: bool` → `TcsLevel { Off, Half, Full }` (legacy bool accepted: `false` → Off, `true` → Full). This is the already-approved row 105 change; the request is about naming: **Half / Full** as in the reference game (row 105 says Low / High) | clip 4 is driven with TCS Half. The parity rows `launch_*_half` read "n/a" until it exists | A (implements), wp_wasm, testbed + ui-kit (3-way control), simcli scenario/replay assists, wp_net | requested |

## Changes outside my crates
- `crates/wp_vehicle/examples/parity.rs`: a new example, no library code changed.
- `justfile`: `parity` recipe.

## Perf numbers
| Measurement | Machine | Native | Browser | Budget |
|---|---|---|---|---|
| `just parity` (8 cars, ~250 manoeuvres) | cloud container, 1 core | 7.3 s release | — | < 2 min |

## Known gaps
- **Gate (2026-10-20, `lead/wf-parity` @ merge of the UI work + `lead/round12`):** fmt ok; clippy
  `-D warnings` clean except `wp_audio/src/siren.rs:437` (digit grouping, already on
  `lead/round12`, not touched here); wasm build ok; tsc ok (both apps); ui-kit vitest 96/96;
  e2e `controls_settings`, `menu`, `presets`, `gamepad` pass. Red, and the same on `lead/round12`:
  - `cargo test --workspace`: simcli `city_siren_traffic_no_siren` (known, D) and five
    `damage_arcade_*` golden hash mismatches. This branch changes no simulation code.
  - `e2e/run.mjs` → `perf_check`: steady allocation 620–730 KB/frame here against 670–690 KB
    on `lead/round12`, run on the same machine (budget ≤ 200). The real-time factor and long
    frames are borderline on software GL.
  - `reset.mjs` failed once inside the full run and passed twice on its own (flaky under
    load).
- TCS Half isn't measurable yet (WP-1).
- The countersteer drift row uses a script equal to Drift Lab C's "Naive" driver, not the lab's
  own code (not merged).
- Reference numbers are few, and some are rough or qualitative (braking is derived from a
  101 → 65 mph stop with downshifts; the tap target "≈ a lane change" is qualitative). The
  table marks every source.
- The sensitivity curve shape (`1 + 2·(1 − sens)`) is our guess. It is the identity at the
  owner's 100 %.
