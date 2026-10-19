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

# Headless server (websocket on the given port, default 8080).
server port="8080":
    cargo run -p wp_server --bin wp-server -- --port {{port}}

# Run one headless scenario, e.g. `just sim apps/simcli/scenarios/smoke/straight_line.ron`.
sim scenario:
    cargo run --release -p simcli -- {{scenario}}

# Regenerate JSON Schemas into schemas/ (integration lead only).
schemas:
    cargo run -p wp_contracts --bin gen-schemas

# Regenerate the debug box lattices in data/lattices/.
lattices:
    cargo run -p wp_damage --bin gen-box-lattice -- --id debug_sedan --width 1.80 --length 4.70 --height 1.45 --wheelbase 2.70 --mass 350 --out data/lattices/debug_sedan.json
    cargo run -p wp_damage --bin gen-box-lattice -- --id debug_coupe --width 1.85 --length 4.50 --height 1.32 --wheelbase 2.65 --mass 375 --out data/lattices/debug_coupe.json

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
