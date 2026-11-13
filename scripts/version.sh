#!/usr/bin/env bash
# Read or write the product version in every place that carries it.
#
#   scripts/version.sh get
#   scripts/version.sh set 0.3.0
#   scripts/version.sh bump patch|minor|major
#
# The version lives in three files and they must stay in step: the workspace
# Cargo.toml, the Tauri config (which names the artifacts) and package.json.
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
cargo_toml="$root/Cargo.toml"
tauri_conf="$root/app/src-tauri/tauri.conf.json"
pkg_json="$root/app/package.json"

semver_re='^[0-9]+\.[0-9]+\.[0-9]+$'

get() {
  sed -n 's/^version = "\([0-9.]*\)"$/\1/p' "$cargo_toml" | head -1
}

write() {
  local v=$1
  [[ $v =~ $semver_re ]] || { echo "not a semver: $v" >&2; exit 1; }
  # Only the [workspace.package] line carries the dependency-free version.
  python3 - "$cargo_toml" "$v" <<'PY'
import sys, re
path, version = sys.argv[1], sys.argv[2]
text = open(path).read()
text = re.sub(r'(?m)^version = "[0-9.]+"$', f'version = "{version}"', text, count=1)
open(path, "w").write(text)
PY
  python3 - "$tauri_conf" "$v" <<'PY'
import json, sys
path, version = sys.argv[1], sys.argv[2]
data = json.load(open(path))
data["version"] = version
open(path, "w").write(json.dumps(data, indent=2) + "\n")
PY
  python3 - "$pkg_json" "$v" <<'PY'
import json, sys
path, version = sys.argv[1], sys.argv[2]
data = json.load(open(path))
data["version"] = version
open(path, "w").write(json.dumps(data, indent=2) + "\n")
PY
  # Cargo.lock records the workspace versions; keep it consistent without a build.
  cargo update --workspace --offline >/dev/null 2>&1 || true
  echo "$v"
}

case "${1:-get}" in
  get)
    get
    ;;
  set)
    [ $# -eq 2 ] || { echo "usage: $0 set X.Y.Z" >&2; exit 1; }
    write "$2"
    ;;
  bump)
    part=${2:-patch}
    cur=$(get)
    [[ $cur =~ ^([0-9]+)\.([0-9]+)\.([0-9]+)$ ]] || { echo "current version is not semver: $cur" >&2; exit 1; }
    major=${BASH_REMATCH[1]} minor=${BASH_REMATCH[2]} patch=${BASH_REMATCH[3]}
    case "$part" in
      major) major=$((major + 1)); minor=0; patch=0 ;;
      minor) minor=$((minor + 1)); patch=0 ;;
      patch) patch=$((patch + 1)) ;;
      *) echo "usage: $0 bump patch|minor|major" >&2; exit 1 ;;
    esac
    write "$major.$minor.$patch"
    ;;
  *)
    sed -n '2,8p' "$0"
    exit 1
    ;;
esac
