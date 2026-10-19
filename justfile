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

# Headless server (websocket on the given port, default 8080; admin on 8081).
server port="8080":
    cargo run -p wp_server --bin wp-server -- --port {{port}}

# Server load test: N scripted websocket bots in the crash arena (see apps/server/README.md).
server-bots n="32":
    cargo run --release -p wp_server --bin wp-server -- --bots {{n}} --arena 120

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
