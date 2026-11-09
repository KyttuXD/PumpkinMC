#!/usr/bin/env bash
# Fails if a Mach-O binary needs a newer macOS than MACOSX_DEPLOYMENT_TARGET,
# or links a non-system dynamic library.
#
# Usage: check_macos_min.sh <binary>
set -euo pipefail

BIN="$1"
WANT="${MACOSX_DEPLOYMENT_TARGET:?MACOSX_DEPLOYMENT_TARGET must be set}"

file "$BIN"
lipo -info "$BIN"

# LC_BUILD_VERSION (minos) on modern toolchains, LC_VERSION_MIN_MACOSX (version) on old ones.
MINOS=$(otool -l "$BIN" | awk '
  /cmd LC_BUILD_VERSION/      { b = 1; next }
  /cmd LC_VERSION_MIN_MACOSX/ { v = 1; next }
  b && $1 == "minos"   { print $2; exit }
  v && $1 == "version" { print $2; exit }
')
if [ -z "$MINOS" ]; then
  echo "::error::Could not read the minimum macOS version of $BIN"
  exit 1
fi
echo "$BIN: minimum macOS $MINOS (allowed: $WANT)"

# Version compare: fail when MINOS > WANT.
if [ "$(printf '%s\n%s\n' "$WANT" "$MINOS" | sort -t. -k1,1n -k2,2n -k3,3n | tail -1)" != "$WANT" ]; then
  echo "::error::$BIN requires macOS $MINOS, newer than the target $WANT"
  exit 1
fi

# Every dynamic library must be a system one, so the binary runs on a clean Mac.
otool -L "$BIN"
if otool -L "$BIN" | tail -n +2 | awk '{print $1}' | grep -vE '^(/usr/lib/|/System/Library/)'; then
  echo "::error::$BIN links non-system dynamic libraries (listed above)"
  exit 1
fi
