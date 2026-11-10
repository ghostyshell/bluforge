#!/bin/sh
# Verify the BD-J menu Xlet without a BD-J player.
#
# libbluray on macOS cannot start BD-J (its JVM loads AWT on the calling
# thread, which enters the AppKit loop inside bdj_open). This harness replaces
# the BD-J platform with stubs, runs the real Menu class, drives it with key
# events, captures the window, and checks the playlist it asks to play.
set -eu
cd "$(dirname "$0")/.."

JAVA8_HOME=${JAVA8_HOME:-$(/usr/libexec/java_home -v 1.8 2>/dev/null || true)}
[ -n "$JAVA8_HOME" ] || { echo "no Java 8 JDK found (set JAVA8_HOME)" >&2; exit 1; }

work=${TMPDIR:-/tmp}/keepsake-menu-test
rm -rf "$work"
mkdir -p "$work/classes" "$work/disc"
printf 'Keepsake Atelier|THE TEST DISC\nFirst Episode|0\nSecond Episode|1\nThird Episode|2\n' > "$work/disc/menu.txt"

"$JAVA8_HOME/bin/javac" -source 8 -target 8 -Xlint:-options -d "$work/classes" \
    core/assets/menu/test/stub/javax/tv/xlet/*.java \
    core/assets/menu/test/stub/javax/media/*.java
"$JAVA8_HOME/bin/javac" -source 8 -target 8 -encoding UTF-8 -Xlint:-options \
    -cp "$work/classes" -d "$work/classes" \
    core/assets/menu/src/Menu.java core/assets/menu/test/src/Harness.java

"$JAVA8_HOME/bin/java" -Dkeepsake.menu.root="$work/disc" -cp "$work/classes" \
    Harness "$work" > "$work/out.log" 2>&1

cat "$work/out.log"
grep -q "KEEPSAKE_MENU_INIT entries=3" "$work/out.log"
grep -q "KEEPSAKE_MENU_PLAY bd://1.PLAYLIST:00001" "$work/out.log"
grep -q "HARNESS_RETURNED true" "$work/out.log"
[ -s "$work/menu-1.png" ] && [ -s "$work/menu-2.png" ]
echo "menu ok: rendered, moved selection, requested playlist, returned ($work)"
