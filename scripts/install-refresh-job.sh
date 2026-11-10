#!/bin/bash
# Install (or remove) the weekly download-link refresh as a user launchd agent.
#
# Default: every Sunday at 03:47, wrapped in caffeinate -i so the Mac does not
# idle-sleep during an upload.
#
#   scripts/install-refresh-job.sh
#   scripts/install-refresh-job.sh --time 04:15 --weekday 1
#   scripts/install-refresh-job.sh --repo /path/to/keepsake-atelier
#   scripts/install-refresh-job.sh --uninstall
set -euo pipefail

SELF=$(cd "$(dirname "$0")" && pwd)
REPO=${KEEPSAKE_REPO:-$(cd "$SELF/.." && pwd)}
LABEL=click.keepsakeatelier.weekly-refresh
WEEKDAY=0
HOUR=3
MINUTE=47
UNINSTALL=0
LOAD=1

while [ $# -gt 0 ]; do
  case "$1" in
    --repo) REPO=$2; shift 2 ;;
    --label) LABEL=$2; shift 2 ;;
    --time)
      HOUR=${2%%:*}
      MINUTE=${2##*:}
      shift 2 ;;
    --weekday) WEEKDAY=$2; shift 2 ;;
    --uninstall) UNINSTALL=1; shift ;;
    --no-load) LOAD=0; shift ;;
    -h|--help)
      sed -n '2,12p' "$0"
      exit 0 ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
done

[[ "$HOUR" =~ ^[0-9]+$ && "$MINUTE" =~ ^[0-9]+$ ]] || { echo "bad --time" >&2; exit 2; }
[ "$HOUR" -ge 0 ] && [ "$HOUR" -le 23 ] || { echo "hour 0-23" >&2; exit 2; }
[ "$MINUTE" -ge 0 ] && [ "$MINUTE" -le 59 ] || { echo "minute 0-59" >&2; exit 2; }
[ "$WEEKDAY" -ge 0 ] && [ "$WEEKDAY" -le 7 ] || { echo "weekday 0-7" >&2; exit 2; }

PLIST="$HOME/Library/LaunchAgents/$LABEL.plist"
UID_NUM=$(id -u)

if [ "$UNINSTALL" = "1" ]; then
  launchctl bootout "gui/$UID_NUM/$LABEL" 2>/dev/null || true
  rm -f "$PLIST"
  echo "removed $LABEL"
  exit 0
fi

[ -d "$REPO" ] || { echo "no repo at $REPO" >&2; exit 1; }
[ -f "$REPO/scripts/refresh-release-links.sh" ] || { echo "missing refresh script in $REPO" >&2; exit 1; }
[ -f "$REPO/.gofile-io-token" ] || echo "warning: $REPO/.gofile-io-token is missing" >&2

mkdir -p "$HOME/Library/LaunchAgents"
python3 - "$PLIST" "$LABEL" "$REPO" "$WEEKDAY" "$HOUR" "$MINUTE" <<'PY'
import os, plistlib, sys
path, label, repo, weekday, hour, minute = sys.argv[1:7]
path_env = os.environ.get("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
extra = [
    "/opt/homebrew/bin",
    "/usr/local/bin",
    os.path.expanduser("~/.cargo/bin"),
    os.path.expanduser("~/.local/bin"),
]
seen, parts = set(), []
for p in path_env.split(":") + extra:
    if p and p not in seen:
        seen.add(p)
        parts.append(p)
command = f"cd {repo} && exec ./scripts/refresh-release-links.sh >> .release-refresh.log 2>&1"
data = {
    "Label": label,
    "ProgramArguments": ["/usr/bin/caffeinate", "-i", "/bin/bash", "-c", command],
    "EnvironmentVariables": {
        "PATH": ":".join(parts),
        "HOME": os.path.expanduser("~"),
    },
    "StartCalendarInterval": {
        "Weekday": int(weekday),
        "Hour": int(hour),
        "Minute": int(minute),
    },
    "RunAtLoad": False,
    "ProcessType": "Background",
    "StandardOutPath": os.path.join(repo, ".release-refresh.log"),
    "StandardErrorPath": os.path.join(repo, ".release-refresh.log"),
}
with open(path, "wb") as fh:
    plistlib.dump(data, fh)
print(f"wrote {path}")
PY

if [ "$LOAD" = "1" ]; then
  launchctl bootout "gui/$UID_NUM/$LABEL" 2>/dev/null || true
  launchctl bootstrap "gui/$UID_NUM" "$PLIST"
  echo "loaded $LABEL (weekday $WEEKDAY, $(printf '%02d:%02d' "$HOUR" "$MINUTE"))"
  launchctl print "gui/$UID_NUM/$LABEL" 2>/dev/null | sed -n '1,12p' || true
else
  echo "not loaded (--no-load)"
fi
