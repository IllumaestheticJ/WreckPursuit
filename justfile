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

# Regenerate the debug lattices in data/lattices/ (gen-lattice sedan/coupe profiles).
lattices:
    cargo run -p wp_damage --bin gen-lattice -- --debug

# Crash-barrier calibration table for a car (default debug_sedan).
barrier car="debug_sedan":
    cargo run --release -p wp_damage --bin damage-barrier -- --sweep {{car}}

# Damage perf benchmark (24 lattices).
damage-bench:
    cargo run --release -p wp_damage --bin damage-bench
