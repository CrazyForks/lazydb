#!/bin/sh
set -eu
[ "$#" -eq 2 ] || { printf 'Usage: smoke-artifact.sh BINARY VERSION\n' >&2; exit 2; }
binary=$1
case "$binary" in /*) ;; *) binary="./$binary" ;; esac
# Capture separately so a failing binary cannot be hidden by a successful pipe.
reported=$("$binary" version --json)
printf '%s\n' "$reported" | python3 -c '
import json, sys
value = json.load(sys.stdin)
actual = value.get("version") if isinstance(value, dict) else None
if actual != sys.argv[1]:
    sys.exit(f"artifact version mismatch: expected {sys.argv[1]}, got {actual!r}")
' "$2"
