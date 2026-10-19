# Harness integration notes

- **Workstream / prompt:** 10 — headless harness, determinism & regression testing, benchmarks, CI
- **Branch:** `ws/harness`
- **Crates / folders owned:** `apps/simcli`, `tools/bench`, `tools/lint-determinism`,
  `tools/wasm-determinism`, `tools/ci`, `.github/workflows/`, the test surface of `wp_sim`
- **Contract version built against:** 0.1.0

## Exposes

### Hooks added to `wp_sim` (test surface only — no game logic)

| item | purpose |
|---|---|
| `wp_sim::Systems { vehicle, damage, world, police }`, `SystemId`, `Systems::parse("vehicle,world")`, `Display` | **system registry**: which subsystems `step_fixed` runs |
| `Sim::set_systems(Systems)` / `Sim::systems()` | default `Systems::ALL` (games and the testbed are unaffected). A disabled system's state is frozen: no vehicle step, no damage contacts/effects, frozen clock, no police step |
| `Sim::last_tick_event_count()` | events emitted by the last tick, without draining (flood detection) |
| `Sim::drain_events()` (existing) | **event drain**: the harness drains after every tick, so events are tick-tagged |
| `Sim::node_positions(EntityId)` | lattice nodes in chassis space (crush depth, fuzz bounds) |
| `Sim::entity_hashes() -> Vec<(EntityId, u64)>` | per-entity hash, ascending entity order (names the entity that diverged) |
| `Sim::state_hash()` (changed) + `wp_sim::HASH_SPEC_VERSION = 1` | hash spec below |

### Hash function spec (v1)

All hashing is FNV-1a 64 (`wp_math::StateHasher`, little-endian integer encoding). Floats are
**quantised** before hashing: `round(v as f64 × scale) as i64` (`wp_math::quantize`; NaN →
`i64::MIN`), so the hash compares behaviour, not bit noise, and f64 multiply/round is exact and
identical on every target.

```
state_hash = FNV( u32 HASH_SPEC_VERSION, u64 tick,
                  q(clock seconds_since_midnight, 1e3),
                  u32 vehicle_count,
                  u64 entity_hash(v) for v in vehicles ascending by EntityId )

entity_hash(v) = FNV( u32 entity id,
                      q(pos[0..3], 1e4)          // 0.1 mm
                      q(rot[0..4], 1e5)
                      q(lin_vel[0..3], 1e4), q(ang_vel[0..3], 1e4)
                      q(engine_rpm, 1), i64 gear, q(health, 1e4),   // health from damage
                      for wheel in FL, FR, RL, RR:
                          q(omega, 1e3), q(steer_rad, 1e5), q(compression_m, 1e5), u8 detached
                      u32 node_count, q(node.xyz, 1e4) for every damage node in id order )
```

Stability rules: fixed entity order (ascending id, ids never reused), fixed field order, no
maps iterated. Any change to the hashed fields bumps `HASH_SPEC_VERSION` and requires
re-blessing every golden; the replay verifier reports a hash-spec mismatch explicitly.
Systems should not add fields to the hash themselves — ask here, so the spec stays in one place.

### Files and formats

- Scenario format: `apps/simcli/SCENARIOS.md`. Outputs: `out/<scenario>/{metrics.csv,
  events.jsonl, hashes.txt, summary.json, replay.bin}`.
- `replay.bin` v1 (`apps/simcli/src/replay.rs`): `"WPREPLAY"`, format version, contract version,
  hash spec, systems, scenario RON (setup), tick count, input events
  `(u64 tick, u16 entity index, 5×f32 axes, u8 flags, u8 tool)`, a u64 state hash per tick,
  checkpoints `(u64 tick, per entity: u64 entity hash, pos, lin_vel, health)`.
  **Testbed (04):** please record replays in this format (setup scenario + applied inputs);
  `simcli replay` then verifies them headlessly.
- Golden layout: `apps/simcli/golden/<name>/hashes.txt` (every scenario),
  `apps/simcli/golden/<name>.replay.bin` (`scenarios/golden/*`).
- WASM runner C ABI (`tools/wasm-determinism`): `wp_alloc`, `wp_free`, `wp_run`,
  `wp_result_ptr`, `wp_result_len`. No host imports.
- Perf: `docs/PERF.md` (append-only log), `tools/bench/baselines/<runner class>.json`.

## Needs

| from | what | workaround now |
|---|---|---|
| 12 (lead, `wp_sim`) | vehicles as Rapier bodies + contact extraction | crash/T-bone/spike goldens only check the approach; physics in `expect_pending` |
| 05 (world) | procedural city API, traffic density, routes | warnings; `city/24v_400traffic` bench measures 24 cars + clock only |
| 06 (police) | presence override, exported `PoliceEvent` kind codes (pursuit started, spike deployed, escaped) | `PURSUIT_STARTED_KIND = 1` provisional; golden spike/pursuit use kinds 2/3 as placeholders in `expect_pending` |
| 07 (net) | `wp-server --bots N --minutes M` | nightly soak falls back to `simcli fuzz --minutes 10 --max-vehicles 32` (auto-switches when `--bots` appears in `apps/server/src/main.rs`) |
| 04 (testbed) | record replays in `replay.bin` v1 | golden replays recorded from scripted scenarios |
| 03 (damage) | real `node_positions` / health | `crush_depth` reads 0 |

## Contract change requests

| # | Change (type / field / variant) | Why | Who is affected | Status |
|---|---|---|---|---|
| H1 | `wp_contracts`: `pub const POLICE_PURSUIT_STARTED: u16` (and other `PoliceEvent` kind codes), or a typed police event | `pursuit_started_before` expectation needs a stable kind code | 06, 10 | proposed |
| H2 | Consider moving `Systems` into `wp_contracts` if `wp_net`/server need to toggle systems too | keep one registry | 07, 12 | idea only |

## Changes outside my crates

- `crates/wp_sim` (test surface, which this workstream owns per prompt 10): new
  `src/systems.rs`; `Sim` gains `set_systems`, `systems`, `last_tick_event_count`,
  `node_positions`, `entity_hashes`, `HASH_SPEC_VERSION`; `state_hash` now follows spec v1
  (adds clock, wheels, lattice nodes; previous hashes are not comparable). `step_fixed` checks
  the system flags; with `Systems::ALL` (the default) behaviour is unchanged. README updated.
- `Cargo.toml` (workspace members): `tools/wasm-determinism`, `tools/lint-determinism`, `tools/bench`.
- `justfile`: harness recipes appended (`scenarios`, `goldens`, `bless`, `determinism`,
  `lint-det`, `fuzz`, `bench`, `ci`).
- `.github/workflows/ci.yml` replaced (owned), `nightly.yml`, `contract-guard.yml` added.
- `docs/PERF.md`, `docs/TESTING.md` added. `docs/INTEGRATION.md` "How to run" table not edited
  (lead): suggest adding `just ci`, `just goldens`, `just bless`, `just fuzz`, `just bench`.

## Perf numbers

Cloud container, Intel Xeon @ 2.10 GHz, 4 cores, release, stub systems (from `docs/PERF.md`):

| Measurement | Native | Browser | Budget |
|---|---|---|---|
| `sim_only/24` (one tick, 24 cars) | 4.6 µs → 0.0008 ms/car/frame | not measured | 0.40 ms/car |
| `sim_only/64` | 12.9 µs/tick | — | — |
| `snapshot/encode_24` / `decode_24` (JSON stub codec) | 30 µs / 50 µs | — | — |
| all 10 scenarios, no recording | 0.14 s | — | — |
| native vs wasm compare, all scenarios (incl. builds) | ≈ 2 min | — | — |

## Known gaps

- Physics/damage/police expectations are pending until those systems land (see Needs).
- The perf gate only gates when runner class and CPU model match the baseline; GitHub's shared
  runners change CPUs, so expect some nights to report "not gating". A self-hosted runner class
  would make the gate strict.
- No `tools/bench` numbers in the browser yet (needs a wasm bench harness; the
  `wasm-determinism` module could host one).
- The lint is line-based: it can miss a transcendental called through a function pointer or a
  macro, and it stops at the first `#[cfg(test)]` in a file.

## Lead feedback
(Written by the integration lead.)
