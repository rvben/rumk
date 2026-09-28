#!/usr/bin/env bash
set -euo pipefail

version="${1#v}"
# A version heading is plain, dated, or linked to its comparison:
# "## [1.2.3]", "## [1.2.3] - DATE", or "## [1.2.3](URL) - DATE".
awk -v heading="## [${version}]" '
    $0 == heading || index($0, heading " - ") == 1 || index($0, heading "(") == 1 {
        printing = 1
        next
    }
    printing && /^## \[/ { exit }
    printing && /^\[[^]]+\]: / { exit }
    printing { print }
' CHANGELOG.md
