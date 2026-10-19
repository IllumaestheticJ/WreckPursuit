# tools/wasm-determinism

**Owned by workstream 10.** `simcli`'s runner compiled to `wasm32-unknown-unknown` with a tiny C
ABI (no wasm-bindgen, no host imports), plus a Node driver. CI runs every scenario natively and
in wasm and requires byte-identical `hashes.txt`. This catches f32/transcendental drift between
targets early.

```sh
tools/wasm-determinism/compare.sh                      # all of apps/simcli/scenarios
tools/wasm-determinism/compare.sh path/to/x.ron        # one scenario
# manual:
cargo build --release -p wasm-determinism --target wasm32-unknown-unknown
cargo run --release -p simcli -- bundle > /tmp/bundle.json
node tools/wasm-determinism/run.mjs target/wasm32-unknown-unknown/release/wasm_determinism.wasm /tmp/bundle.json x.ron
```

Needs Node ≥ 18. `run.mjs` stubs every wasm import with a throwing function; today there are
none.

If native and wasm differ, `compare.sh` prints the first lines of the diff (tick of the first
mismatch). Find the transcendental or `f64` path involved and route it through `wp_math`.
Current status: **all scenarios match** (stub systems use `wp_math::sin_cos` only).

## Tests

```sh
cargo test -p wasm-determinism      # native run_hashes() equals the golden hashes.txt
```
