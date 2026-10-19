# WreckPursuit task runner. Install: `cargo install just`. List recipes: `just --list`.

# Run the full native test suite.
test:
    cargo test --workspace

# fmt + clippy (-D warnings) + tests + wasm build, as CI does.
check:
    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo test --workspace
    cargo build --target wasm32-unknown-unknown -p wp_wasm

# Build the wasm core and package it for the browser (needs wasm-pack + npm deps).
wasm:
    cd apps/testbed-web && npm install && npm run wasm

# Browser testbed: build wasm, then start Vite at http://localhost:5173.
web: wasm
    cd apps/testbed-web && npm run dev

# Headless-Chromium end-to-end check of the testbed (drive, layers, replay after reload, scenarios).
web-e2e: wasm
    cd apps/testbed-web && npm run e2e

# Native testbed stub window (add `--features window,ffb` manually for an SDL2 FFB wheel).
native:
    cargo run --release -p testbed-native --features window

# Headless server (websocket on the given port, default 8080).
server port="8080":
    cargo run -p wp_server --bin wp-server -- --port {{port}}

# Meta service (accounts, matchmaking, leaderboards, admin) on 127.0.0.1:8090 with dev secrets
# and fast password hashing. Never use these settings in a deployment (see apps/meta/README.md).
meta:
    WP_META_DB=meta-dev.sqlite WP_META_INSTANCE_KEY=dev-instance-key-000 WP_META_ADMIN_TOKEN=dev-admin-token-0000 WP_META_FAST_HASH=1 cargo run -p wp_meta --bin wp-meta

# Run one headless scenario, e.g. `just sim apps/simcli/scenarios/smoke/straight_line.ron`.
sim scenario:
    cargo run --release -p simcli -- {{scenario}}

# Regenerate JSON Schemas into schemas/ (integration lead only).
schemas:
    cargo run -p wp_contracts --bin gen-schemas

# Regenerate the debug lattices in data/lattices/ (gen-lattice sedan/coupe profiles).
# Regenerate the procedural city (data/city/procedural_12x12.{road.bin,road.json,greybox.glb}).
city:
    cargo run --release -p city-import --bin gen-city -- --grid 12x12 --seed 1 --out data/city

# Traffic benchmark / soak: `just traffic-bench 10` (minutes, 400 agents, noon).
traffic-bench minutes="2":
    cargo run --release -p wp_world --example traffic_bench -- {{minutes}} 400 12

# Regenerate the debug box lattices in data/lattices/.
lattices:
    cargo run -p wp_damage --bin gen-lattice -- --debug

# Crash-barrier calibration table for a car (default debug_sedan).
barrier car="debug_sedan":
    cargo run --release -p wp_damage --bin damage-barrier -- --sweep {{car}}

# Damage perf benchmark (24 lattices).
damage-bench:
    cargo run --release -p wp_damage --bin damage-bench

# --- Test harness (workstream 10, see docs/TESTING.md) ---

# Run every scenario, write out/<scenario>/*, fail on expectations.
scenarios:
    cargo run --release -p simcli -- run apps/simcli/scenarios

# Compare every scenario with its golden hashes / golden replay (diff report on failure).
goldens:
    cargo run --release -p simcli -- goldens

# Regenerate all goldens after an intended physics change; commit with [bless] in the message.
bless:
    cargo run --release -p simcli -- goldens --bless

# Determinism: twice in-process + fresh process, then native vs wasm32 under Node.
determinism:
    cargo run --release -p simcli -- determinism apps/simcli/scenarios
    tools/wasm-determinism/compare.sh

# Grep-based determinism lint over the simulation crates.
lint-det:
    cargo run -p lint-determinism

# Random-input soak with invariant checks (default 5 minutes).
fuzz minutes="5":
    cargo run --release -p simcli -- fuzz --minutes {{minutes}}

# Criterion benches, then append docs/PERF.md and gate against the local baseline.
bench class="local":
    cargo bench -p wp_bench
    cargo run --release -p wp_bench --bin perf-gate -- check --class {{class}} --append docs/PERF.md

# Everything CI runs for the harness, locally.
ci: check lint-det determinism goldens
    tools/ci/data_coverage.sh
