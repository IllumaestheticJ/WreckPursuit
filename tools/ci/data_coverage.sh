#!/usr/bin/env bash
# Lists data/** files that no schema/loader validation covers (wp_contracts' data_validation test
# covers cars, tires, materials, lattices, modes). Prints a CI warning per folder so new data
# folders get a schema request; exits 0 (informational) unless --strict.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
covered='^data/(cars|tires|materials|lattices)/[^/]+\.json$|^data/modes/[^/]+\.toml$|README\.md$|/.gitkeep$'
uncovered=$(git ls-files data | grep -Ev "$covered" || true)
if [ -z "$uncovered" ]; then
  echo "data-coverage: every data file is schema-validated"
  exit 0
fi
for dir in $(dirname $uncovered | sort -u); do
  echo "::warning::$dir has files without a schema (request one from the integration lead)"
done
sed 's/^/  /' <<<"$uncovered"
[ "${1:-}" = "--strict" ] && exit 1
exit 0
