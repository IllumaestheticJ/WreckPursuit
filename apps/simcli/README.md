# simcli

**Owned by workstream 10 (test harness & CI).** Headless scenario runner, determinism suite,
golden hash/replay regression and fuzz/soak for the whole simulation. Scenario format:
[`SCENARIOS.md`](SCENARIOS.md). How workstreams use it: [`docs/TESTING.md`](../../docs/TESTING.md).

## Commands

```sh
cargo run -p simcli -- run apps/simcli/scenarios                  # all scenarios → out/<name>/*
cargo run -p simcli -- run X.ron --systems vehicle,world          # exclude stub systems
cargo run -p simcli -- run X.ron --compare apps/simcli/golden/<name>/hashes.txt
cargo run -p simcli -- run X.ron --bless                          # regenerate X's goldens
cargo run -p simcli -- goldens [--bless]                          # check / bless every golden
cargo run -p simcli -- replay apps/simcli/golden/golden_tbone.replay.bin   # divergence report
cargo run -p simcli -- determinism apps/simcli/scenarios          # 2× in-process + fresh process
cargo run -p simcli -- hashes X.ron                               # hashes.txt to stdout
cargo run -p simcli -- --fuzz --minutes 5 [--seed S]              # soak; repro → out/fuzz/*.ron
cargo run -p simcli -- bundle > bundle.json                       # data root for the WASM runner
```

`simcli <file.ron>` without a subcommand means `run`. `--data DIR` picks the data root (default
`./data`, else the repo's). Exit codes: 0 pass, 1 expectation/golden/determinism/fuzz failure,
2 usage or load error. Use `--release` for anything long (fuzz, soak, many scenarios).

## Layout

| path | what |
|---|---|
| `src/scenario.rs` | scenario types, parsing, validation |
| `src/run.rs` | runner: drivers, metrics, events, hashes, expectations; city, traffic and police setup |
| `src/net.rs` | net round trip on `wp_net`'s wire codec (quantise → delta → `ServerMsg::Snapshot` → decode → re-apply) |
| `examples/probe_police.rs` | prints police unit positions per second, to aim police scenarios at a patrol |
| `src/replay.rs` | `replay.bin` format (documented in the module) + verifier / diff report |
| `src/golden.rs` | golden hashes/replays, compare, bless (+ `[bless]` CI guard) |
| `src/determinism.rs` | in-process repeat check |
| `src/fuzz.rs` | random cases + invariants (NaN, node bounds, below ground, event floods) |
| `src/data.rs` | one loader path for disk and in-memory (WASM) data |
| `scenarios/` | scenario library (`smoke/`, `regression/`, `vehicle/`, `golden/`, `soak/`) |
| `golden/` | `<name>/hashes.txt` for every scenario, `<name>.replay.bin` for `scenarios/golden/*` |

## Tests

```sh
cargo test -p simcli
```

Runs every scenario (expectations + in-process determinism), the fresh-process check, golden
comparison, output files, `--compare`, replay divergence reporting, the CI bless guard and a short
fuzz. `cargo test --workspace` therefore fails when a change alters any golden.

## Perf (harness overhead)

All 10 library scenarios (≈ 21 000 ticks, up to 24 stub cars) run in ≈ 0.14 s release without
recording on the cloud container (Xeon 2.1 GHz). Fuzz: ≈ 20 simulated seconds × up to 16 cars per case; see
`docs/PERF.md` for sim numbers.

## Known gaps

- Police units are `wp_police`'s internal kinematic cars (no physics bodies), so PIT/ramming
  and spike strips don't touch the player's physics yet. The spike-strip golden runs on the
  flat world (no police), and the night-pursuit golden's racer passes its "patrol" (a plain
  parked car) too fast to be recognised. Both keep their police targets in `expect_pending`.
  `golden_city_chase` is the working pursuit golden.
- `follow_route` and `ai_driver` warn and fall back. Traffic in scenarios is kinematic.
- `pursuit_started_before` uses a provisional police kind code (`PURSUIT_STARTED_KIND = 1`)
  until the typed `PoliceEvent` (contract v0.2.0) lands.
- City props (parked cars) are physics bodies but are not part of the state hash.
- Golden replays are recorded from scripted scenarios; the testbed (prompt 04) should record
  real-driver replays in the same format (`src/replay.rs`).
