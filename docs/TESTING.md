# Testing guide

How every workstream proves its work with the shared harness (workstream 10). Short version:
**write a scenario with ranges, run it, bless the goldens when behaviour changes on purpose.**

## What CI runs

| workflow / job | what fails it |
|---|---|
| `ci.yml` → `rust` | `cargo fmt --check`, `clippy -D warnings`, `cargo test --workspace` (includes every scenario, golden comparison, fresh-process determinism, the determinism lint audit, schema drift), wasm build of `wp_wasm` |
| `ci.yml` → `schema` | any `data/**` file failing its JSON Schema or typed loader; files with no schema are reported as warnings |
| `ci.yml` → `workstream-checks` | `wp_economy` tests with the native SQLite store |
| `ci.yml` → `determinism-lint` | `f64`, libm transcendentals, `HashMap`, wall clock, threads or unseeded RNG in sim crates |
| `ci.yml` → `determinism` | a scenario whose hashes differ between two in-process runs, a fresh process, or **native vs wasm32** (Node) |
| `ci.yml` → `goldens` | a scenario whose `hashes.txt` or golden replay differs from `apps/simcli/golden/` (prints first divergent tick, entity, magnitude) |
| `ci.yml` → `harness-report` | never fails; writes to the job summary every `#[ignore]`d test with its reason (bare `#[ignore]` flagged "no reason given") and every release-only scenario (`soak/`, `slow: true`). Locally: `just report` |
| `ci.yml` → `bless-guard` (PRs) | golden or perf-baseline files changed without a `[bless]` commit |
| `ci.yml` → `fuzz` | any invariant break in 1 minute of random scenarios |
| `contract-guard.yml` (PRs) | `crates/wp_contracts/` or `schemas/` changed without a `CONTRACT_VERSION` bump and a `## v<version>` entry in `docs/CONTRACT_CHANGELOG.md` |
| `nightly.yml` | benches > 20 % slower than the blessed baseline for the runner class; 10-minute 32-bot soak; 5-minute fuzz |

Run it all locally with `just ci` (or the individual `just` recipes below).

## Adding a scenario

1. Pick a folder under `apps/simcli/scenarios/` (`vehicle/`, `damage/`, `world/`, `police/`, …;
   `golden/` is reserved for the "feel" library, see below; anything that simulates more than
   ~20 s of a full city goes in `soak/`, or sets `slow: true` if it belongs in another folder;
   debug `cargo test` skips both and the release CI jobs run them, including expectations, in
   `goldens`). File name = what it tests.
   Parking a Rust test? Use `#[ignore = "<who> (<round>): why, and what un-ignores it"]`, never a
   bare `#[ignore]`: the harness report lists the reason in CI so parked tests stay visible.
2. Write it (format: `apps/simcli/SCENARIOS.md`). Rules:
   - **Ranges, not exact numbers** (`speed_at(t_s: 5.0, min: 20.0, max: 24.0)`). Exact behaviour
     is already pinned by the golden hash; the ranges say what is *physically right*.
   - Use `assists: ai` or `off` for physics tests (keyboard filtering changes the inputs).
   - Use `driver: cruise(target_mps: …)` to reach a test speed exactly.
   - Targets your system cannot meet yet go in `expect_pending`; move them to `expect` when
     they pass. Pending results are printed on every run.
   - If your crate is a stub that would make the scenario meaningless, exclude it:
     `systems: "vehicle,world"`.
3. Run it: `cargo run -p simcli -- run apps/simcli/scenarios/<folder>/<file>.ron` and look at
   `out/<name>/summary.json` and `metrics.csv`.
4. Bless its goldens: `cargo run -p simcli -- run <file> --bless` (writes
   `apps/simcli/golden/<name>/hashes.txt`).
5. Commit scenario + golden with `[bless]` in the message, e.g.
   `wp_vehicle: brake 100-0 scenario [bless]`.

`cargo test -p simcli` picks the scenario up automatically (expectations, determinism, golden).

## When a golden fails

A failing golden means **the simulation's exact behaviour changed**. CI prints, per scenario (illustrative output, numbers made up):

```
FAIL golden_tbone (apps/simcli/scenarios/golden/tbone.ron)
  hashes: first mismatch at tick 24: golden 4e12…, now 9a07…
  replay: first divergent tick: 1 (0.0042 s); diverged entities at next checkpoint: ["striker"]
  replay: first checkpoint @ tick 24: striker |Δpos| 0.003100 m, |Δvel| 0.120000 m/s, Δhealth +0.000000
  replay: final @ tick 1440: striker |Δpos| 4.210000 m, …
```

- Unintended (you did not mean to touch that system) → it is a bug or a determinism leak; fix it.
  `cargo run -p simcli -- replay apps/simcli/golden/<name>.replay.bin` reproduces the report.
- Intended (you tuned physics) → check the scenario **expectations still pass** (they are the
  real acceptance criteria), then bless:

```sh
cargo run --release -p simcli -- goldens --bless      # or: just bless
git commit -am "wp_vehicle: stiffer rear springs [bless]"
```

Blessing in CI (`simcli --bless` with `CI` set) is refused unless the commit message contains
`[bless]`, and the `bless-guard` job fails a PR that changes goldens without such a commit.
Mention blessed goldens in your integration doc so the lead can sanity-check the change.

## Golden replays ("feel" library)

`apps/simcli/scenarios/golden/*.ron` each get `apps/simcli/golden/<name>.replay.bin`: setup +
recorded inputs + a hash for every tick + periodic state checkpoints (format in
`apps/simcli/src/replay.rs`). Verifying a replay re-applies the *recorded inputs* (not the
scenario's drivers), so the testbed can record human-driven replays in the same format.
Current library: FWD lift-off, RWD power slide, 56 km/h frontal, T-bone, spike strip, night
pursuit escape. Owners of the relevant system tighten their `expect_pending` targets.

## Determinism

- Hash spec: `docs/integration/harness.md` (quantised state, fixed entity order).
- `just determinism`: two in-process runs + a fresh process + native vs wasm32 under Node.
- If native and wasm differ: the first differing line in `hashes.txt` gives the tick; replay it
  natively and in wasm and look for a transcendental (`sin`, `atan2`, `powf`, `exp`) or `f64`
  path. Route it through `wp_math` (request a software implementation there) — the
  determinism lint (`just lint-det`) flags most of these statically.
- Sim code: no wall clock, no `HashMap` iteration, no unseeded RNG, no threads (see `CLAUDE.md`).
  Suppress a lint false positive with `// det-allow(<rule>): <reason>` on the line.

## Fuzz / soak

`cargo run --release -p simcli -- --fuzz --minutes 5 [--seed S]` generates random scenarios
(random inputs, cars aimed at each other head-on and side-on) and checks every tick: no NaN or
infinity anywhere in vehicle/wheel state or lattice nodes, unit quaternions, no lattice node
more than 0.5 m outside the rest bounding box, no chassis below z = −0.5 m, at most 1000
events per tick. A failure writes `out/fuzz/<case>.ron`, a normal scenario that reproduces it
with `simcli run`.

## Benchmarks

`just bench` runs criterion (`tools/bench`) and appends a fingerprinted table to `docs/PERF.md`.
Nightly CI gates against `tools/bench/baselines/gha-ubuntu-latest.json` (20 % threshold, only
when the CPU model matches). Accept new numbers: download the nightly `perf` artifact's
`gha-ubuntu-latest.json` into `tools/bench/baselines/` and commit with `[bless]`. Budgets are in
`CLAUDE.md`; record your crate's numbers in its README too.

## Contract changes

Never edit `crates/wp_contracts` or `schemas/` on a workstream branch; request changes in your
integration doc. `contract-guard.yml` enforces the version bump + changelog for the lead.
