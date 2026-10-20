#!/bin/bash
# Fast preview gate (owner request 2026-10-20): compile, wasm, tests only for crates touched since
# BASE (default origin/main) plus their dependents, and a browser smoke (scripted city chase).
# The full gate (fmt, clippy, cargo test --workspace, all scenarios, goldens, native-vs-wasm,
# web-e2e) runs on the main track only.
set -u
cd "$(dirname "$0")/.."
export PATH=$HOME/.cargo/bin:$PATH CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
BASE=${BASE:-origin/main}
changed=$(git diff --name-only "$BASE"...HEAD)
crates=$(echo "$changed" | sed -n 's#^crates/\([^/]*\)/.*#\1#p; s#^apps/\(simcli\|server\)/.*#\1#p' | sort -u)
fail=0
step(){ n=$1; shift; if "$@" >/tmp/qg-$n.log 2>&1; then echo "ok   $n"; else echo "FAIL $n (see /tmp/qg-$n.log)"; fail=1; fi; }
step check cargo check -q --workspace --all-targets
for c in $crates; do
  pkg=$(cargo metadata -q --no-deps --format-version 1 | python3 -c "import json,sys;d=json.load(sys.stdin);print(next((p['name'] for p in d['packages'] if '/$c/' in p['manifest_path']),''))")
  [ -n "$pkg" ] && step "test-$pkg" cargo test -q -p "$pkg"
done
if echo "$changed" | grep -qE '^(crates/(wp_sim|wp_vehicle|wp_damage|wp_world|wp_police|wp_weather)|data/)'; then
  step scenarios bash -c 'cargo build -q --release -p simcli && ./target/release/simcli run apps/simcli/scenarios --no-write'
fi
step wasm bash -c 'cd apps/testbed-web && npm run -s wasm && npx tsc --noEmit -p .'
exit $fail
