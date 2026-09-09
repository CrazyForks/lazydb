#!/bin/sh
set -eu

[ "$#" -eq 1 ] || { printf 'Usage: check-macos-dependencies.sh BINARY\n' >&2; exit 2; }
binary=$1
[ -f "$binary" ] || { printf 'binary not found: %s\n' "$binary" >&2; exit 1; }

output=$(otool -L "$binary") || {
    printf 'otool failed for: %s\n' "$binary" >&2
    exit 1
}

dependencies=$(printf '%s\n' "$output" | awk 'NR > 1 { sub(/^[[:space:]]+/, ""); sub(/[[:space:]]+\(compatibility version.*$/, ""); print }')
[ -n "$dependencies" ] || {
    printf 'no dynamic dependencies found for: %s\n' "$binary" >&2
    exit 1
}

failed=0
while IFS= read -r dependency; do
    case "$dependency" in
        /usr/lib/*|/System/Library/*) ;;
        *)
            printf 'non-system macOS dependency: %s\n' "$dependency" >&2
            failed=1
            ;;
    esac
done <<EOF
$dependencies
EOF

if [ "$failed" -ne 0 ]; then
    printf 'macOS binary is not portable: %s\n' "$binary" >&2
    exit 1
fi
printf 'macOS dependencies are system-only: %s\n' "$binary"
