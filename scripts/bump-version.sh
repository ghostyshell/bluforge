#!/usr/bin/env bash
# Cut a version: move the Unreleased changelog section under a dated heading and
# bump the version everywhere.
#
#   scripts/bump-version.sh patch|minor|major
#
# Minor is the default for the release flow: features go in minor, fixes in
# patch, and major is reserved for a change that breaks saved state.
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
changelog="$root/CHANGELOG.md"
part=${1:-minor}

cd "$root"

python3 - "$changelog" <<'PY'
import sys
text = open(sys.argv[1]).read()
head, sep, tail = text.partition("## Unreleased\n")
if not sep:
    sys.exit("CHANGELOG.md has no '## Unreleased' section")
# Only the part before the next '## ' heading belongs to this release.
body, sep2, rest = tail.partition("\n## ")
if body.strip() == "":
    sys.exit("the Unreleased section is empty; write the entry first")
open(sys.argv[1], "w").write(head + sep + body + sep2 + rest)
PY

cur=$(scripts/version.sh get)
case "$part" in
  major) next="$(( ${cur%%.*} + 1 )).0.0" ;;
  minor) rest=${cur#*.}; next="${cur%%.*}.$(( ${rest%%.*} + 1 )).0" ;;
  patch) next="${cur%.*}.$(( ${cur##*.} + 1 ))" ;;
  *) echo "usage: $0 patch|minor|major" >&2; exit 1 ;;
esac
if grep -q "^## $next - " "$changelog"; then
  echo "CHANGELOG.md already has a section for $next; released twice?" >&2
  exit 1
fi

version=$(scripts/version.sh bump "$part")
date=$(date +%F)

python3 - "$changelog" "$version" "$date" <<'PY'
import sys
path, version, date = sys.argv[1:]
text = open(path).read()
text = text.replace(
    "## Unreleased\n",
    f"## Unreleased\n\nNothing yet.\n\n## {version} - {date}\n",
    1,
)
open(path, "w").write(text)
PY

printf 'version %s\n' "$version"
printf 'CHANGELOG.md now has a %s section\n' "$version"
printf 'next: scripts/check-changelog.sh, then commit both together\n'
