# tools/bench (`wp_bench`)

**Owned by workstream 10.** Criterion benchmarks and the perf gate.

| bench id | measures (one iteration) |
|---|---|
| `sim_only/{1,8,24,64}` | one `Sim::step_fixed` with N cars (vehicle + damage + world + police) |
| `city/24v_400traffic` | one tick with 24 cars + world; traffic agents join when prompt 05 lands (id kept stable) |
| `lattice/solver_8_contacts` | one `DamageSim::apply_contacts` with 8 frontal impulses |
| `snapshot/encode_24`, `snapshot/decode_24` | one 24-car `ServerToClient::Snapshot` through `wp_net` |

```sh
cargo bench -p wp_bench                                            # criterion → target/criterion
cargo run --release -p wp_bench --bin perf-gate -- report          # table only
cargo run --release -p wp_bench --bin perf-gate -- check --class local --append docs/PERF.md
cargo run --release -p wp_bench --bin perf-gate -- bless --class local
```

`perf-gate` converts per-tick medians to per-frame numbers (60 Hz = 4 ticks, per car where the
budget is per car), appends a fingerprinted section (runner class, CPU, cores, platform, rustc,
commit, date) to `docs/PERF.md`, and with `check` fails when a bench is more than 20 %
(`--threshold`) **and** at least 100 ns slower than `baselines/<class>.json`. It only gates when
the baseline's runner class and CPU model match this machine; otherwise it reports and passes.
`--class` defaults to `$WP_RUNNER_CLASS`, else `local`. Nightly CI uses `gha-ubuntu-latest` and
uploads a baseline candidate; commit it with `[bless]` to accept new numbers.

## Tests

```sh
cargo test -p wp_bench
```

## Numbers

See `docs/PERF.md`. Stub systems today: `sim_only/24` ≈ 4.6 µs per tick on the cloud container.

## Known gaps

- No browser (wasm) benches yet.
- `city/24v_400traffic` has no traffic until `wp_world` exposes it.
