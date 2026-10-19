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
