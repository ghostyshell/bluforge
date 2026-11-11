#!/bin/sh
# Render and drive the authored BD-J menu without a Blu-ray player.
#
# Usage:
#   scripts/test-menu.sh [disc-tree] [output-dir]
#
# With a disc tree, it runs the JAR and menu data that would ship on the disc
# (BDMV/JAR/00000.jar + menu.txt + optional menu.png). Without one, it builds a
# three-entry synthetic menu so the Xlet and capture path can be checked on a
# clean machine.
#
# What it proves: the real Xlet starts, draws a frame, responds to Down, plays
# the selected playlist through javax.media, and returns to the menu on
# EndOfMedia. It writes menu-1.png, menu-2.png and menu-3.png.
#
# What it does not prove: a hardware player's BD-J implementation, BDJO
# navigation, or HDMI output. `keepsake-atelier verify` checks the BDJO and
# playlist side.
set -eu
cd "$(dirname "$0")/.."

DISC=${1:-}
OUT=${2:-${TMPDIR:-/tmp}/keepsake-menu-test}

if [ -n "${JAVA8_HOME:-}" ] && [ -x "$JAVA8_HOME/bin/javac" ]; then
    JAVAC="$JAVA8_HOME/bin/javac"
    JAVA="$JAVA8_HOME/bin/java"
elif command -v javac >/dev/null 2>&1 && command -v java >/dev/null 2>&1; then
    JAVAC=$(command -v javac)
    JAVA=$(command -v java)
elif [ -x /usr/libexec/java_home ]; then
    JAVAC="$(/usr/libexec/java_home -v 1.8 2>/dev/null)/bin/javac"
    JAVA="$(/usr/libexec/java_home -v 1.8 2>/dev/null)/bin/java"
else
    echo "no JDK found: set JAVA8_HOME or put javac/java on PATH" >&2
    exit 1
fi
[ -x "$JAVAC" ] || { echo "javac not executable: $JAVAC" >&2; exit 1; }

rm -rf "$OUT"
mkdir -p "$OUT/classes"

if [ -n "$DISC" ]; then
    JAR_DIR="$DISC/BDMV/JAR"
    [ -f "$JAR_DIR/menu.txt" ] || {
        echo "no menu.txt under $JAR_DIR" >&2
        exit 1
    }
    echo "menu root: $JAR_DIR"
else
    JAR_DIR="$OUT/disc"
    mkdir -p "$JAR_DIR"
    printf 'Keepsake Atelier|THE TEST DISC\nFirst Episode|0\nSecond Episode|1\nThird Episode|2\n' \
        > "$JAR_DIR/menu.txt"
    echo "menu root: synthetic $JAR_DIR"
fi

"$JAVAC" -source 8 -target 8 -Xlint:-options -d "$OUT/classes" \
    core/assets/menu/test/stub/javax/tv/xlet/*.java \
    core/assets/menu/test/stub/javax/media/*.java
# Prefer the actual shipped JAR. Fall back to compiling the Xlet source when
# the asset is not built in this checkout.
CP="$OUT/classes"
if [ -f core/assets/menu/00000.jar ]; then
    CP="core/assets/menu/00000.jar:$CP"
    echo "menu jar: core/assets/menu/00000.jar"
else
    "$JAVAC" -source 8 -target 8 -encoding UTF-8 -Xlint:-options \
        -cp "$OUT/classes" -d "$OUT/classes" core/assets/menu/src/Menu.java
    echo "menu jar: compiled from source"
fi

"$JAVAC" -source 8 -target 8 -encoding UTF-8 -Xlint:-options \
    -cp "$CP" -d "$OUT/classes" \
    core/assets/menu/test/src/Harness.java

"$JAVA" -Dkeepsake.menu.root="$JAR_DIR" -cp "$CP" Harness "$OUT" > "$OUT/out.log" 2>&1 || {
    cat "$OUT/out.log" >&2
    exit 1
}

cat "$OUT/out.log"
grep -q "KEEPSAKE_MENU_INIT entries=" "$OUT/out.log" || {
    echo "menu never initialised" >&2
    exit 1
}
grep -q "HARNESS_PLAYER " "$OUT/out.log" || {
    echo "menu never requested a playlist" >&2
    exit 1
}
grep -q "HARNESS_RETURNED true" "$OUT/out.log" || {
    echo "menu did not return after playback" >&2
    exit 1
}
[ -s "$OUT/menu-1.png" ] || { echo "no initial frame" >&2; exit 1; }
[ -s "$OUT/menu-2.png" ] || { echo "no frame after navigation" >&2; exit 1; }
[ -s "$OUT/menu-3.png" ] || { echo "no frame after returning" >&2; exit 1; }

echo "menu ok: rendered, navigated, requested a playlist, returned"
echo "frames: $OUT/menu-1.png $OUT/menu-2.png $OUT/menu-3.png"
