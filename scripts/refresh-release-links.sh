#!/bin/bash
# Weekly entry point for the release mirror refresh.
#
# The real work lives in publish-release.sh: rebuild the current version,
# re-upload to GoFile and file.kiwi, repoint docs/downloads.json and the README
# table, commit, and remove the local artifacts. This wrapper exists so launchd
# can call one stable path.
set -euo pipefail
SELF=$(cd "$(dirname "$0")" && pwd)
REPO=${KEEPSAKE_REPO:-$(cd "$SELF/.." && pwd)}

cd "$REPO"
printf '[%s] refresh start (%s)\n' "$(date '+%F %T')" "$REPO"
exec "$REPO/scripts/publish-release.sh" "$@"
