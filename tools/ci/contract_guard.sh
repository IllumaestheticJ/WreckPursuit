#!/usr/bin/env bash
# Contract guard: a change to crates/wp_contracts/ or schemas/ must come with
#   1. a bumped wp_contracts::CONTRACT_VERSION (strictly greater, semver order), and
#   2. a docs/CONTRACT_CHANGELOG.md change containing a "## v<new version>" heading.
# Usage: tools/ci/contract_guard.sh <base-ref>     (e.g. origin/main)
set -euo pipefail
base=${1:?usage: contract_guard.sh <base-ref>}
cd "$(git rev-parse --show-toplevel)"

lib=crates/wp_contracts/src/lib.rs
changed=$(git diff --name-only "$base"...HEAD)
contract_changes=$(grep -E '^(crates/wp_contracts/|schemas/)' <<<"$changed" || true)
if [ -z "$contract_changes" ]; then
  echo "contract-guard: no changes to wp_contracts or schemas/"
  exit 0
fi
echo "contract-guard: contract files changed:"
sed 's/^/  /' <<<"$contract_changes"

version_of() { grep -E '^pub const CONTRACT_VERSION' | sed -E 's/.*"([^"]+)".*/\1/'; }
old=$(git show "$base:$lib" 2>/dev/null | version_of || true)
new=$(version_of < "$lib")
fail=0
if [ -z "$new" ]; then
  echo "::error file=$lib::CONTRACT_VERSION not found"; exit 1
fi
if [ -n "$old" ]; then
  highest=$(printf '%s\n%s\n' "$old" "$new" | sort -V | tail -n1)
  if [ "$old" = "$new" ] || [ "$highest" != "$new" ]; then
    echo "::error file=$lib::CONTRACT_VERSION must be bumped above $old (is $new)"
    fail=1
  fi
fi
if ! grep -qx 'docs/CONTRACT_CHANGELOG.md' <<<"$changed"; then
  echo "::error file=docs/CONTRACT_CHANGELOG.md::contract changed without a docs/CONTRACT_CHANGELOG.md entry"
  fail=1
elif ! grep -qE "^## v${new//./\\.}([^0-9]|$)" docs/CONTRACT_CHANGELOG.md; then
  echo "::error file=docs/CONTRACT_CHANGELOG.md::no '## v$new' heading for the new version"
  fail=1
fi
[ $fail -eq 0 ] && echo "contract-guard: ok ($old -> $new, changelog entry present)"
exit $fail
