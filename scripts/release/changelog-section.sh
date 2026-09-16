#!/bin/sh
set -eu
[ "$#" -eq 1 ] || { printf 'Usage: changelog-section.sh VERSION\n' >&2; exit 2; }
version=$1
awk -v version="$version" '
    $0 ~ "^## \\[" version "\\] - [0-9]{4}-[0-9]{2}-[0-9]{2}$" { found=1; print; next }
    found && /^## / {
        exit
    }
    found && /^### Commits$/ {
        print
        print ""
        print "<details>"
        print "<summary>Show commits</summary>"
        print ""
        commits=1
        next
    }
    found && commits && /^### / {
        print "</details>"
        print ""
        commits=0
    }
    found { print }
    END {
        if (found && commits) print "</details>"
    }
' CHANGELOG.md
