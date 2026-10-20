#!/usr/bin/env bash
# Debug vs release determinism: build simcli in the dev profile and the release profile from the
# current checkout, run each scenario in both (`simcli hashes`), and require byte-identical
# hashes.txt output. Replays, goldens and multiplayer assume the build profile never changes the
# simulation.
#   tools/profile-determinism/compare.sh [scenario.ron|dir ...]
# Default: the steering-heavy quick set below (vehicle/, smoke/, cars/ run in well under a second
# each; flee_police adds AI steering + ESC with traffic and police, ~10 s in the dev profile).
#
# Both binaries are always rebuilt here. Never compare a `target/*/simcli` you did not just build:
# a release binary left over from before a merge is the usual cause of a "debug != release"
# report (docs/integration/harness.md, "Lead helper: debug vs release hashes").
set -euo pipefail
cd "$(dirname "$0")/../.."
targets=("$@")
[ ${#targets[@]} -eq 0 ] && targets=(
  apps/simcli/scenarios/vehicle
  apps/simcli/scenarios/smoke
  apps/simcli/scenarios/cars
  apps/simcli/scenarios/opponent/flee_police.ron
)
mapfile -t files < <(find "${targets[@]}" -name '*.ron' | sort)

cargo build -q -p simcli
cargo build -q --release -p simcli
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

fail=0
for f in "${files[@]}"; do
  ./target/debug/simcli hashes "$f" --data data > "$work/debug.txt"
  ./target/release/simcli hashes "$f" --data data > "$work/release.txt"
  if diff -u "$work/debug.txt" "$work/release.txt" > "$work/diff.txt"; then
    echo "ok   $f debug == release"
  else
    echo "FAIL $f: debug and release hashes differ (first lines of diff):"
    head -n 12 "$work/diff.txt"
    fail=1
  fi
done
[ $fail -eq 0 ] && echo "profile determinism: ${#files[@]} scenarios identical in debug and release"
exit $fail
