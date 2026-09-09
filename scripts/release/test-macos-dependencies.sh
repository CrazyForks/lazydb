#!/bin/sh
set -eu
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT HUP INT TERM

mkdir -p "$TMP/bin"
printf '%s\n' binary > "$TMP/binary"
cat > "$TMP/bin/otool" <<'SH'
#!/bin/sh
set -eu
case "${OTOOL_CASE:-system}" in
system)
    printf '%s\n' "$1:"
    printf '\t%s\n' '/usr/lib/libSystem.B.dylib (compatibility version 1.0.0, current version 1.0.0)' '/System/Library/Frameworks/AppKit.framework/Versions/C/AppKit (compatibility version 1.0.0, current version 1.0.0)'
    ;;
homebrew)
    printf '%s\n' "$1:"
    printf '\t%s\n' '/opt/homebrew/opt/xz/lib/liblzma.5.dylib (compatibility version 14.0.0, current version 14.3.0)'
    ;;
macports)
    printf '%s\n' "$1:"
    printf '\t%s\n' '/opt/local/lib/liblzma.5.dylib (compatibility version 14.0.0, current version 14.3.0)'
    ;;
loader)
    printf '%s\n' "$1:"
    printf '\t%s\n' '@rpath/liblzma.5.dylib (compatibility version 14.0.0, current version 14.3.0)'
    ;;
empty)
    printf '%s\n' "$1:"
    ;;
failed)
    exit 1
    ;;
esac
SH
chmod 755 "$TMP/bin/otool"

PATH="$TMP/bin:$PATH" sh "$ROOT/scripts/release/check-macos-dependencies.sh" "$TMP/binary"
for case_name in homebrew macports loader empty failed; do
    if OTOOL_CASE="$case_name" PATH="$TMP/bin:$PATH" \
        sh "$ROOT/scripts/release/check-macos-dependencies.sh" "$TMP/binary" >/dev/null 2>&1; then
        printf 'dependency case was incorrectly accepted: %s\n' "$case_name" >&2
        exit 1
    fi
done
if PATH="$TMP/bin:$PATH" sh "$ROOT/scripts/release/check-macos-dependencies.sh" "$TMP/missing" >/dev/null 2>&1; then
    printf '%s\n' 'missing binary was incorrectly accepted' >&2
    exit 1
fi
printf '%s\n' 'macOS dependency checks passed'
