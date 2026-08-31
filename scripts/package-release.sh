#!/usr/bin/env bash
set -euo pipefail

if [[ "$#" -ne 3 ]]; then
    echo "usage: $0 <target> <version> <tar.gz|zip>" >&2
    exit 2
fi

target="$1"
version="$2"
format="$3"
extension=""
if [[ "${format}" == "zip" ]]; then
    extension=".exe"
fi

binary="target/${target}/release/rumk${extension}"
if [[ ! -f "${binary}" ]]; then
    echo "release binary not found: ${binary}" >&2
    exit 1
fi

package="rumk-${version}-${target}"
staging="$(mktemp -d)"
trap 'rm -rf "${staging}"' EXIT
mkdir -p "${staging}/${package}" dist
cp "${binary}" README.md CHANGELOG.md LICENSE rumk.schema.json "${staging}/${package}/"

case "${format}" in
    tar.gz)
        tar -C "${staging}" -czf "dist/${package}.tar.gz" "${package}"
        ;;
    zip)
        (
            cd "${staging}"
            7z a -bd -tzip "${OLDPWD}/dist/${package}.zip" "${package}" >/dev/null
        )
        ;;
    *)
        echo "unsupported package format: ${format}" >&2
        exit 2
        ;;
esac
