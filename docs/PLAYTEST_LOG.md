# Playtest log (owner feedback)

## 2026-10-19 — first city chase on the published link
| # | Feedback | Owner of the fix | Status |
|---|---|---|---|
| 1 | No city buildings on the published link | lead (artifact can't serve .glb → base64 fallback in `scene.ts`) | fixed, link v5 |
| 2 | No visible car damage | lead: lattice view on by default; real denting panels = R (prompt 21) | interim fix on link |
| 3 | Cars feel really slow | helper `lead/playtest-feel`: browser sim perf (sim fell behind real time at ~35–50 ms/frame in city chase) + car speed/feel | fixed on `lead/playtest-feel` (numbers below); not yet on the link |
| 4 | Transmission is sloppy | helper `lead/playtest-feel` (shift logic) | fixed on `lead/playtest-feel` (numbers below); not yet on the link |
| 5 | Use real open-source sounds | J (audio): CC0 recorded packs | dispatched |
| 6 | Dispatcher and cops are way too talkative | helper `lead/playtest-radio` | dispatched |
| 7 | Camera zooms way too far out; need more angles | helper `lead/playtest-camera` | dispatched |

### Rows 3–4: before / after (`lead/playtest-feel`, 2026-10-19, cloud container)

**Row 3 — slow motion.** Cause: frames slower than 100 ms lost sim time (the frame loop capped
catch-up at 0.1 s), and the city chase cost more per tick than it needed. Fixes: 0.25 s
catch-up; 3,300 city props fixed until a car comes near; no per-tick police car clone; faster
(same-value) replay hash; lattice views rebuilt only when dented. Determinism unchanged (all
35 scenario hashes identical across the perf commit; native == wasm 35/35).

| City chase (debug_sedan, Arcade, W held) | Before | After |
|---|---|---|
| Native sim, ms per 60 Hz frame: mean / p95 / worst | 3.1 / 4.8 / 73 | **1.2 / 2.2 / 4.6** |
| — Rapier (props) / police / replay hash / vehicles | 0.92 / 0.41 / 0.83 / 0.51 | 0.15 / 0.01 / 0.40 / 0.34 |
| — first 0.5 s of a chase | 23 ms per frame (props awake) | no spike |
| Browser (wasm) sim, ms per 60 Hz frame | 3.5–4.2 | **2.6–3.1** |
| Browser JS views (lattice/skin), ms per frame | 16.4 | **4.8–5.3** |
| Real-time factor, headless 1280×720 (CPU raster, 3–6 fps) | **0.35** | **0.90–0.94** |
| Real-time factor, headless 480×270 | — | 0.96–0.99 |

Headless Chromium rasterises on the CPU (SwiftShader), so its fps is not the owner's; with a
GPU the JS work per frame is ~2.8 (sim) + ~5 (views) + ~5 (render submit) ms. The HUD now
shows `real time 1.00×` and an amber "⚠ slow motion" warning below 0.95×.

**Row 3 — the car.** City chase / free drive default to **Lindwurm 4T** (AWD, 80 ms dual-clutch,
0–100 5.4 s, in its 4.6–5.4 band) instead of the debug sedan (8.3 s). Keyboard throttle: JS
ramp 0.2 → 0.08 s and the sim's keyboard filter 0.44 → 0.23 s (they were stacked).

0–100 km/h (`wp_vehicle` metrics, release; TCS on; band from `data/cars/VALIDATION.md`):

| Car | Band | Before | After |
|---|---|---|---|
| Lucciola 350 | 4.3–5.0 | 4.57 | 4.55 |
| Debug Coupe | (5–6) | 5.23 | 5.20 |
| Lindwurm 4T (new default) | 4.6–5.4 | 5.40 | 5.42 |
| Sentinel Wolfhound | 5.5–6.5 | 5.55 | 5.58 |
| Rampart 5.0 | 4.5–5.0 | 5.93 ✗ | 5.84 ✗ |
| Hakone GT | 4.8–5.8 | 5.84 ✗ | 5.86 ✗ |
| Sperber 32 | 5.0–5.8 | 6.06 ✗ | 6.05 ✗ |
| Shiro Turbo | 5.6–6.6 | 6.30 | 6.42 |
| Kestrel TS | 6.2–7.2 | 7.14 | 7.08 |
| Debug Sedan (old default) | (8–10) | 8.13 | 8.29 |
| Vireo XS | 8.5–9.5 | 9.20 | 9.45 |

Launches are traction/TCS-limited (TCS off is slower for every 2WD car), so the gearbox barely
moves WOT times. The old numbers carried a small unphysical bonus: the old box let the engine
flare to the limiter in every shift and then dumped the clutch (worth ~4 km/h per shift on the
Vireo). The three ✗ cars were already out of band (H's VALIDATION notes: TCS-conservative).

**Row 4 — sloppy transmission.** Old automatic: drive cut for the whole shift with the clutch
open, engine flared to the limiter (6,772 rpm on the sedan, every shift), then a 0.1 s clutch
re-ramp; at 35% pedal it still revved to 88% of redline before upshifting; at half throttle
the schedule could hunt (1→2→1). New automatic (`crates/wp_vehicle/README.md`):

| Check | Before | After |
|---|---|---|
| Shift (sedan / Hakone, 0.25 s) | full cut 0.25 s + 0.1 s clutch ramp, engine flare to limiter | release → rev-matched neutral (0.1 s no drive) → in-sync re-engage; engine 6,500 → 3,070 rpm before the clutch closes |
| Hunting at constant pedal | possible (sedan at 50% pedal: the 1→2 upshift landed below the downshift point) | none at any pedal 0–1 / any speed (unit test) and over a 0.2–1.0 throttle sweep on 4 cars (rig test) |
| Upshift point at 35% pedal | 88% of redline (5,700 rpm sedan) | ~57% (load-based; 0.36 light … 0.975 full) |
| Kickdown | one gear when rpm fell below the point | pedal stab → lowest gear ≤ 85% redline, skipping gears (≥ 2 gears in one shift from a 70 km/h cruise, tested) |
| Mid-corner at the limit | shifts | held (lateral usage ≥ 0.75) |
| Braking | one gear per 0.65 s | sequential, never above 72% redline, held 1.5 s after the brake |
| Standard launch (WOT) | — | sedan 2 upshifts by 10 s, Hakone 2 by 8 s, 0 downshifts (tests) |
