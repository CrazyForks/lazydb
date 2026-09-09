#!/bin/sh
set -eu
SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
python3 -B "$SCRIPT_DIR/test-next-version.py"
for test in test-archive test-macos-dependencies test-installer test-channel-manifest test-pages test-release-metadata test-online-install; do
    printf 'Running %s\n' "$test"
    sh "$SCRIPT_DIR/$test.sh"
done
