#!/usr/bin/env bash
# Writes SHA256SUMS for every file in a release directory and verifies it.
set -euo pipefail

if [[ "$#" -ne 1 ]]; then
    echo "usage: $0 <directory>" >&2
    exit 2
fi

cd "$1"
assets=()
while IFS= read -r name; do
    assets+=("${name}")
done < <(find . -maxdepth 1 -type f ! -name SHA256SUMS | sed 's|^\./||' | LC_ALL=C sort)
if [[ "${#assets[@]}" -eq 0 ]]; then
    echo "no release archives in $1" >&2
    exit 1
fi

# GNU coreutils on Linux runners, shasum on macOS; both write and check the same format.
if command -v sha256sum >/dev/null; then
    sha256=(sha256sum)
else
    sha256=(shasum -a 256)
fi
"${sha256[@]}" "${assets[@]}" > SHA256SUMS
"${sha256[@]}" --check SHA256SUMS
