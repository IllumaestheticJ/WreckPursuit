# Vehicle physics lead — `lead/physics`

Owner of handling (brief: `docs/briefs/vehicle-physics-lead.md`). Branch from `lead/round12`.

## 2026-10-22 — start
- Baseline `held_search ONLY_BASE=1`: held 2.24 s, yaw_ok 0.35, taps 20 / 36, handling fails 0.
- **Shipped player rack already shrinks lock with speed:** a held key gets 84 % / 65 % / 49 % of
  full lock at 60 / 100 / 140 km/h (grip-balance measurement). With no speed reduction
  (`RACK=full`) the held drift drops to 2.13 s, taps 12 / 36.
- grip-balance (session_01FKfdoiNS5WT1M85cD1WAu9) asked to push its lock-fixed re-search and
  hand off.

## Reference rack vs speed (owner question: "is it speed scaled?")
Speedometer read from the 16:30 / 16:34 clips (`tools/ref-clips/speedo_glyphs.py` segments the
digits; 10 hand-labelled templates; 0.5 s median; read on 61–72 % of frames). Held steer keys,
≥ 0.3 s into the hold; "past view" = wheel marker beyond ~70° (hidden by the dash, i.e. near or
at full lock; the wheel's full-lock marker angle is unknown, > 90° on parity_rwd_f).

| speed | rwd_e past view | rwd_f past view | visible held angle |
|---|---|---|---|
| < 48 km/h | 29 % | 94 % | 19–48° (low-speed taps / parking) |
| 48–97 km/h | 86–90 % | 100 % | — |
| 97–121 km/h | 36 % | 96 % | 25–62° |
| 121–145 km/h | 0 % | 52 % | 46–59° |
| > 145 km/h | — | 0 % (n = 3) | ~58° |

Reading: **no visible speed reduction up to ~110–120 km/h; above that a held key stops near
~50–60° of wheel marker** (some reduction; the exact share of full lock is unknown because full
lock is out of view). Ours already cuts to 65 % at 100 km/h, earlier and harder than the
reference.

## Touched outside my crates (to compile)
- `wp_police` (round12 8757f12f did not compile after PR #42 met the row-144 radio tiers):
  `race.rs` `RadioTier::Critical` → `Escalation` (its documented successor), and
  `SpikeStrip::new` sets `scripted: false` (as the other constructor). `cargo test -p wp_police`
  green. The lead should take its own fix over this one if it differs.

## Rack A/B + lock-fixed re-search (2026-10-22)
- Tune panel buttons (`?tune=1` / F10): **rack · shipped** (0.75, 113 km/h, p 2), **rack ·
  reference** (0.75, 172 km/h, p 4: 98 / 92 / 89 / 75 / 70 % at 60 / 100 / 110 / 140 / 150
  km/h), **rack · full lock**. Steering only. Replaces the rejected candidate-D button.
- `held_search` with the rack fixed (`RACK=ref|full`, also forces car-wide lock 1.0):

| rack | held | yaw_ok | taps / 36 | handling fails |
|---|---|---|---|---|
| shipped | 2.24 s | 0.35 | 20 | 0 |
| reference | 2.15 s | 0.33 | 12 | 0 |
| full lock | 2.13 s | 0.33 | 12 | 0 |

- **Lock-fixed search, reference rack, 4 seeds × 150 (tyres, spin model, relaxation, grip
  balance, yaw inertia; no lock):** best held 2.68 s, and every candidate above 2.5 s breaks
  the guard rails (taps 1–7 / 36, handling fails up to 15). The only clean one (cand 136,
  handling 0) holds 2.59 s with taps 4 / 36. **No lock-preserving tyre / balance / inertia
  setting gets near 3 s.** The search's old gains came from cutting lock.
- Next (row 148): the levers the owner named. TCS Half holding the wheelspin and the hands-off
  countersteer (30 % cap) on the held drift, then front slip-angle behaviour. The bench drives
  TCS Off today (`reference_assists`).

## grip-balance handoff merged (lead/grip-balance e6a591de)
grip-balance stopped handling work and handed over (its results: `docs/integration/grip-balance.md`,
last section; logs in `docs/integration/grip-balance-held-search/`). Its `held_search` (lock
fixed, lock-at-speed guard, springs / bars / roll inertia, `FRONT=1` front tyre shape) replaces
mine; `RACK=ref` added. Same conclusion from 4 more seeds: best within every guard rail 2.59 s
(taps 21, handling 0, shipped rack: b0 6.48, b2 1.75, c 0.90, floor 0.38, relax 1.41, peak 1.10,
rear 0.533, front 0.666, yaw 1.35, spring 1.04, arb 0.63, roll 1.34; AI pace not run). Front tyre
shape breaks handling. Their open hunches: the clip marker is the steering wheel, not the road
wheels; front slip-angle limit in a closing slide; rack compliance; aligning moment on the body.

## Assist levers on the held drift (row 148), `held_search` `TCS=` / `CS=` / `ENDS=1`
Base tyres, RACK=ref unless named; held s / taps of 36:

| TCS | CS 0 | CS 0.3 |
|---|---|---|
| Off | 2.15 / 12 | 2.15 / 11 |
| Half | 1.95 / **24** | 1.95 / 24 |
| Full | 1.86 / 1 | 1.86 / 1 |

- The hands-off countersteer help cannot act on a *held* drift (it only acts with no key).
- **TCS Half doubles the tapping driver's held entries (12 → 24 / 36)**, in line with the owner's
  "far more controllable" on TCS Half in the reference. The held-key bench gets a little shorter.
- **How held drifts end (`ENDS=1`, 12 runs):** never a spin past 90°. ~75 % **swap / snap back**
  (the slide closes through zero to the other side) after 2.2–3.6 s, the rest stall (car goes
  backward) about 1 s after a low-speed entry. Shipped rack + TCS Half: swaps average 3.55 s
  (Off: 2.63 s); reference rack: 2.2 s either way.
- Full lock is 38° (rwd_a/d/e, rampart) or 33° (rwd_b/f), inside the 30–45° drift band, so as the
  slide closes below the lock angle a held full lock points the fronts past the travel and helps
  pull the nose through (grip-balance's trace). Next: trace the swap (front vs rear force at the
  moment it closes) before choosing a tyre-side change.
