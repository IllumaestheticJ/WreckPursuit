#!/usr/bin/env bash
# Bless guard: golden hashes/replays (apps/simcli/golden/) and perf baselines
# (tools/bench/baselines/) may only change in a PR if one of its commits carries "[bless]" in its
# message. This keeps physics changes that alter goldens deliberate and visible in history.
# Usage: tools/ci/bless_guard.sh <base-ref>
set -euo pipefail
base=${1:?usage: bless_guard.sh <base-ref>}
cd "$(git rev-parse --show-toplevel)"

blessed=$(git diff --name-only "$base"...HEAD -- apps/simcli/golden tools/bench/baselines)
if [ -z "$blessed" ]; then
  echo "bless-guard: no golden or baseline changes"
  exit 0
fi
# Read the whole log before grepping: `git log | grep -q` under pipefail fails when grep exits
# early and git log dies of SIGPIPE, which happens on long histories (e.g. a big merge).
messages=$(git log --format=%B "$base"..HEAD)
if grep -qF '[bless]' <<<"$messages"; then
  echo "bless-guard: goldens changed and a commit is tagged [bless]:"
  sed 's/^/  /' <<<"$blessed"
  exit 0
fi
echo "::error::goldens/baselines changed without a [bless] commit. If intended, regenerate with"
echo "::error::'cargo run -p simcli -- goldens --bless' and commit with [bless] in the message:"
sed 's/^/  /' <<<"$blessed"
exit 1
