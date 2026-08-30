#!/usr/bin/env bash
set -euo pipefail

version="${1#v}"
awk -v heading="## [${version}]" '
    $0 == heading || index($0, heading " - ") == 1 { printing = 1; next }
    printing && /^## \[/ { exit }
    printing && /^\[[^]]+\]: / { exit }
    printing { print }
' CHANGELOG.md
