#!/usr/bin/env bash
# Native vs WASM determinism: run every scenario natively (`simcli hashes`) and inside the
# wasm32 build under Node, and require byte-identical hashes.txt output.
#   tools/wasm-determinism/compare.sh [scenario.ron|dir ...]   (default: apps/simcli/scenarios)
set -euo pipefail
cd "$(dirname "$0")/../.."
targets=("$@")
[ ${#targets[@]} -eq 0 ] && targets=(apps/simcli/scenarios)
mapfile -t files < <(find "${targets[@]}" -name '*.ron' | sort)

cargo build --release -q -p simcli
cargo build --release -q -p wasm-determinism --target wasm32-unknown-unknown
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
./target/release/simcli bundle --data data > "$work/bundle.json"
wasm=target/wasm32-unknown-unknown/release/wasm_determinism.wasm

fail=0
for f in "${files[@]}"; do
  ./target/release/simcli hashes "$f" --data data > "$work/native.txt"
  if ! node tools/wasm-determinism/run.mjs "$wasm" "$work/bundle.json" "$f" > "$work/wasm.txt"; then
    echo "FAIL $f: wasm run errored"; fail=1; continue
  fi
  if diff -u "$work/native.txt" "$work/wasm.txt" > "$work/diff.txt"; then
    echo "ok   $f native == wasm"
  else
    echo "FAIL $f: native and wasm hashes differ (first lines of diff):"
    head -n 12 "$work/diff.txt"
    fail=1
  fi
done
exit $fail
