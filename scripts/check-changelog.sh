#!/usr/bin/env bash
# Fail when a change ships without a version bump and a changelog entry.
#
#   scripts/check-changelog.sh            # against origin/main
#   scripts/check-changelog.sh <base>     # explicit base commit
#
# Product changes are anything under core/, cli/, app/ (excluding dist and test
# snapshots) and the docs that describe behaviour. The version must differ from
# the base and CHANGELOG.md must carry a section for the new version, so a
# change can never land as "0.1.0" again and again.
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"

base=${1:-}
if [ -z "$base" ]; then
  base=$(git rev-parse --abbrev-ref --symbolic-full-name '@{u}' 2>/dev/null || true)
  base=${base:-origin/main}
fi
git rev-parse --verify --quiet "$base" >/dev/null || {
  echo "unknown base '$base'" >&2
  exit 2
}

# Compare the base against the working tree, so uncommitted work is covered too.
changed=$(git diff --name-only "$base")
[ -n "$changed" ] || { echo "no changes against $base"; exit 0; }

product=$(printf '%s\n' "$changed" | grep -E '^(core|cli|app/src|app/src-tauri/src|docs/guides/|README\.md|Cargo\.toml)' || true)
if [ -z "$product" ]; then
  echo "no product files changed; no bump required"
  exit 0
fi

old=$(git show "$base:Cargo.toml" | sed -n 's/^version = "\([0-9.]*\)"$/\1/p' | head -1)
new=$(scripts/version.sh get)

fail=0
if [ "$old" = "$new" ]; then
  echo "product files changed but the version is still $new" >&2
  fail=1
fi

if ! grep -q "^## $new - " CHANGELOG.md; then
  echo "CHANGELOG.md has no '## $new - <date>' section" >&2
  fail=1
fi

if [ "$fail" -ne 0 ]; then
  cat >&2 <<'MSG'

Write the entry under '## Unreleased', then:
  scripts/bump-version.sh minor    # or patch / major
MSG
  exit 1
fi

echo "version $old -> $new with a changelog entry"
