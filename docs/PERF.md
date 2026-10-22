# Performance log

Appended by `perf-gate` (`tools/bench`); newest entries at the bottom. Each entry has its
machine fingerprint. Compare numbers only within one **runner class** (and CPU model); the
nightly gate fails a bench that is more than 20 % slower than the blessed baseline in
`tools/bench/baselines/<runner class>.json`.

How to add an entry and gate locally:

```sh
cargo bench -p wp_bench                                   # or: just bench
cargo run -p wp_bench --bin perf-gate -- check --class local --append docs/PERF.md
cargo run -p wp_bench --bin perf-gate -- bless --class local   # accept the current numbers
```

Every bench measures **one physics tick** (240 Hz) unless it is a snapshot op; "per frame" is
4 ticks (60 Hz). Budgets (native, release) are from `CLAUDE.md`; browser targets are 2×.
`sim_only/*` covers vehicle + damage + world + police together, so its budget is
vehicle 0.15 + lattice 0.25 = 0.40 ms per car. Until prompts 02/03/05 land the systems are
stubs and these numbers only prove the harness works.


## 2026-10-19 — `ca9b0fe` @ claude-cloud

- runner class: `claude-cloud`
- cpu: Intel(R) Xeon(R) Processor @ 2.10GHz (4 logical cores), linux-x86_64
- rustc 1.97.0 (2d8144b78 2026-07-07)

| bench | median / iter | per frame (60 Hz) | budget (native) | vs baseline |
|---|---|---|---|---|
| `city/24v_400traffic` | 4.45 µs | 0.0178 ms/frame | 11.6 ms (24 × 0.40 + traffic 2.0) | +0.0% |
| `lattice/solver_8_contacts` | 0.8 ns | 0.0000 ms/car/frame | 0.25 ms/car | +0.0% |
| `sim_only/1` | 490.4 ns | 0.0020 ms/car/frame | 0.40 ms/car (vehicle 0.15 + lattice 0.25) | +0.0% |
| `sim_only/24` | 4.57 µs | 0.0008 ms/car/frame | 0.40 ms/car (vehicle 0.15 + lattice 0.25) | +0.0% |
| `sim_only/64` | 12.93 µs | 0.0008 ms/car/frame | 0.40 ms/car (vehicle 0.15 + lattice 0.25) | +0.0% |
| `sim_only/8` | 1.65 µs | 0.0008 ms/car/frame | 0.40 ms/car (vehicle 0.15 + lattice 0.25) | +0.0% |
| `snapshot/decode_24` | 49.88 µs | 0.0499 ms/snapshot | — | +0.0% |
| `snapshot/encode_24` | 30.36 µs | 0.0304 ms/snapshot | — | +0.0% |

## 2026-10-19 — `cef7f7f` @ claude-cloud

- runner class: `claude-cloud`
- cpu: Intel(R) Xeon(R) Processor @ 2.10GHz (4 logical cores), linux-x86_64
- rustc 1.97.0 (2d8144b78 2026-07-07)

| bench | median / iter | per frame (60 Hz) | budget (native) | vs baseline |
|---|---|---|---|---|
| `city/24v_400traffic` | 166.66 µs | 0.6666 ms/frame | 11.6 ms (24 × 0.40 + traffic 2.0) | +3643.1% |
| `lattice/solver_8_contacts` | 0.9 ns | 0.0000 ms/car/frame | 0.25 ms/car | +16.8% |
| `sim_only/1` | 9.04 µs | 0.0362 ms/car/frame | 0.40 ms/car (vehicle 0.15 + lattice 0.25) | +1743.6% |
| `sim_only/24` | 173.54 µs | 0.0289 ms/car/frame | 0.40 ms/car (vehicle 0.15 + lattice 0.25) | +3700.4% |
| `sim_only/64` | 519.81 µs | 0.0325 ms/car/frame | 0.40 ms/car (vehicle 0.15 + lattice 0.25) | +3921.6% |
| `sim_only/8` | 49.88 µs | 0.0249 ms/car/frame | 0.40 ms/car (vehicle 0.15 + lattice 0.25) | +2928.5% |
| `snapshot/decode_24` | 65.21 µs | 0.0652 ms/snapshot | — | +30.7% |
| `snapshot/encode_24` | 47.78 µs | 0.0478 ms/snapshot | — | +57.4% |

> Lead note (integration 2026-10-19): the entry above is the first with the real `wp_vehicle`
> model, Rapier chassis bodies and contact extraction (damage, world and police are still stubs).
> The "regressions" are against the stub baseline and expected: 24 cars cost 0.029 ms/car/frame,
> ≈ 7 % of the 0.40 ms/car budget. Snapshot encode/decode grew because the JSON stub codec now
> carries non-trivial floats. Baseline `claude-cloud.json` re-blessed in the same `[bless]` commit.

## 2026-10-19 — `b1fd87f` @ claude-cloud

- runner class: `claude-cloud`
- cpu: Intel(R) Xeon(R) Processor @ 2.10GHz (4 logical cores), linux-x86_64
- rustc 1.97.0 (2d8144b78 2026-07-07)

| bench | median / iter | per frame (60 Hz) | budget (native) | vs baseline |
|---|---|---|---|---|
| `city/24v_400traffic` | 391.58 µs | 1.5663 ms/frame | 11.6 ms (24 × 0.40 + traffic 2.0) | +135.0% |
| `lattice/solver_8_contacts` | 145.45 µs | 0.5818 ms/car/frame | 0.25 ms/car | +15351520.8% |
| `sim_only/1` | 6.31 µs | 0.0252 ms/car/frame | 0.40 ms/car (vehicle 0.15 + lattice 0.25) | -30.2% |
| `sim_only/24` | 382.58 µs | 0.0638 ms/car/frame | 0.40 ms/car (vehicle 0.15 + lattice 0.25) | +120.5% |
| `sim_only/64` | 2.77 ms | 0.1729 ms/car/frame | 0.40 ms/car (vehicle 0.15 + lattice 0.25) | +432.2% |
| `sim_only/8` | 36.87 µs | 0.0184 ms/car/frame | 0.40 ms/car (vehicle 0.15 + lattice 0.25) | -26.1% |
| `snapshot/decode_24` | 52.94 µs | 0.0529 ms/snapshot | — | -18.8% |
| `snapshot/encode_24` | 40.17 µs | 0.0402 ms/snapshot | — | -15.9% |

## 2026-10-19 — `867d9b5` @ claude-cloud

- runner class: `claude-cloud`
- cpu: Intel(R) Xeon(R) Processor @ 2.10GHz (4 logical cores), linux-x86_64
- rustc 1.97.0 (2d8144b78 2026-07-07)

| bench | median / iter | per frame (60 Hz) | budget (native) | vs baseline |
|---|---|---|---|---|
| `city/24v_400traffic` | 562.46 µs | 2.2498 ms/frame | 11.6 ms (24 × 0.40 + traffic 2.0) | +43.6% |
| `lattice/solver_8_contacts` | 168.72 µs | 0.6749 ms/car/frame | 0.25 ms/car | +16.0% |
| `sim_only/1` | 8.72 µs | 0.0349 ms/car/frame | 0.40 ms/car (vehicle 0.15 + lattice 0.25) | +38.2% |
| `sim_only/24` | 418.41 µs | 0.0697 ms/car/frame | 0.40 ms/car (vehicle 0.15 + lattice 0.25) | +9.4% |
| `sim_only/64` | 3.25 ms | 0.2034 ms/car/frame | 0.40 ms/car (vehicle 0.15 + lattice 0.25) | +17.6% |
| `sim_only/8` | 47.18 µs | 0.0236 ms/car/frame | 0.40 ms/car (vehicle 0.15 + lattice 0.25) | +28.0% |
| `snapshot/decode_24` | 1.59 µs | 0.0016 ms/snapshot | — | -97.0% |
| `snapshot/encode_24` | 3.27 µs | 0.0033 ms/snapshot | — | -91.9% |

## 2026-10-19 — weather in the game (lead, `lead/weather-in-game`) @ claude-cloud

Hand measurements (not perf-gate benches). Budgets: weather has no row in `CLAUDE.md`; it rides
inside the frame next to traffic (2 ms) and police (0.5 ms).

| What | Native (release) | Browser (wasm, headless Chromium) | Notes |
|---|---|---|---|
| Weather sim, 12×12 city, golden city-chase layout | ≈ 0.04–0.08 ms / 60 Hz frame (1200 frames: 1.01–1.07 s dry vs 1.09–1.32 s in heavy rain, 3 runs each) | — | grid update every 0.25 s sim + per-wheel `weather_at` |
| Weather sim, flat world, 1 car, 62 500-cell grid | — | 0.24 ms vs 0.14 ms / frame without weather | was 7.0 ms before hashing per revision (the browser hashes every tick) |
| K's `weather_bench` (40 000 cells + 400 vehicles) | 0.20 ms per internal update = 0.013 ms / frame | — | `crates/wp_weather/README.md` |
| `weather_info()` / `weather_grid()` | — | 0.005 ms per frame / 0.8 ms at 1 Hz | `e2e/weather.mjs` |
| Rain + spray rendering (5000 streaks, ≤ 1500 spray points) | — | SwiftShader 1280×720: `scene.render` 196 ms vs 132–149 ms without (rain only 191, spray only 189, 1 streak 142) | software GPU: fill-rate bound and not representative; the rain is one instanced draw animated on the GPU with no per-frame CPU work, spray one Points draw. Measure on a real GPU at the next owner playtest |
