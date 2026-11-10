#!/bin/sh
# Build the generic BD-J menu Xlet (00000.jar) used by `author --menu`.
#
# Needs a Java 8 JDK (libbluray's BD-J runtime rejects Java 9+) and the two
# libbluray runtime JARs, which Homebrew's libbluray does not ship. Build them
# from the libbluray source once:
#
#   curl -LO https://downloads.videolan.org/pub/videolan/libbluray/1.5.0/libbluray-1.5.0.tar.xz
#   tar -xf libbluray-1.5.0.tar.xz
#   # compile src/libbluray/bdj/{java,java-j2se,java-build-support} plus
#   # contrib/asm, then jar them as libbluray-j2se-1.5.0.jar and
#   # libbluray-awt-j2se-1.5.0.jar (see docs/roadmap.md).
#
# Environment: JAVA8_HOME, LIBBLURAY_J2SE (jar), LIBBLURAY_AWT (jar).
set -eu
cd "$(dirname "$0")/.."

JAVA8_HOME=${JAVA8_HOME:-$(/usr/libexec/java_home -v 1.8 2>/dev/null || true)}
LIBBLURAY_J2SE=${LIBBLURAY_J2SE:-/tmp/libbluray-j2se-1.5.0.jar}
LIBBLURAY_AWT=${LIBBLURAY_AWT:-/tmp/libbluray-awt-j2se-1.5.0.jar}

[ -n "$JAVA8_HOME" ] || { echo "no Java 8 JDK found (set JAVA8_HOME)" >&2; exit 1; }
[ -f "$LIBBLURAY_J2SE" ] || { echo "missing $LIBBLURAY_J2SE" >&2; exit 1; }
[ -f "$LIBBLURAY_AWT" ] || { echo "missing $LIBBLURAY_AWT" >&2; exit 1; }

out=core/assets/menu
rm -rf "$out/classes"
mkdir -p "$out/classes"
"$JAVA8_HOME/bin/javac" -source 8 -target 8 -encoding UTF-8 -Xlint:-options \
    -cp "$LIBBLURAY_J2SE:$LIBBLURAY_AWT" \
    -d "$out/classes" "$out/src/Menu.java"
"$JAVA8_HOME/bin/jar" cf "$out/00000.jar" -C "$out/classes" .
rm -rf "$out/classes"
mkdir -p app/src-tauri/resources
cp "$out/00000.jar" app/src-tauri/resources/00000.jar
echo "wrote $out/00000.jar"
