#!/usr/bin/env bash
# Publication steps of a tagged release, run in order by the Release workflow:
#
#   draft     create the draft GitHub release with the assets, or refresh a draft
#             left by an earlier attempt; a public release keeps its assets
#   crate     publish to crates.io unless the version is already there
#             (needs CARGO_REGISTRY_TOKEN)
#   pypi-dist collect the wheels and the source distribution for PyPI
#   finalize  make the GitHub release public
#
# The GitHub steps need GH_TOKEN.
set -euo pipefail

if [[ "$#" -ne 5 ]]; then
    echo "usage: $0 <draft|crate|pypi-dist|finalize> <tag> <version> <prerelease:true|false> <dist-dir>" >&2
    exit 2
fi

step="$1"
tag="$2"
version="$3"
prerelease="$4"
dist="$5"

visibility=(--latest)
if [[ "${prerelease}" == "true" ]]; then
    visibility=(--prerelease --latest=false)
fi

case "${step}" in
    draft)
        notes="$(mktemp)"
        trap 'rm -f "${notes}"' EXIT
        ./scripts/release-notes.sh "${version}" > "${notes}"
        if [[ ! -s "${notes}" ]]; then
            echo "release notes are empty for ${version}" >&2
            exit 1
        fi
        if state="$(gh release view "${tag}" --json isDraft --jq .isDraft 2>/dev/null)"; then
            if [[ "${state}" == "true" ]]; then
                gh release upload "${tag}" "${dist}"/* --clobber
                gh release edit "${tag}" --title "Rumk ${tag}" --notes-file "${notes}"
            else
                echo "release ${tag} is already public; preserving its assets"
            fi
        else
            gh release create "${tag}" "${dist}"/* --draft --verify-tag --title "Rumk ${tag}" \
                --notes-file "${notes}" "${visibility[@]}"
        fi
        ;;
    crate)
        endpoint="https://crates.io/api/v1/crates/rumk/${version}"
        if curl --fail --silent --show-error --user-agent "rumk-release-workflow" --output /dev/null "${endpoint}"; then
            echo "rumk ${version} is already on crates.io; skipping publication"
        else
            cargo publish --locked
        fi
        ;;
    pypi-dist)
        rm -rf pypi-dist
        mkdir pypi-dist
        cp "${dist}"/*.whl "${dist}/rumk-${version}.tar.gz" pypi-dist/
        ;;
    finalize)
        gh release edit "${tag}" --draft=false "${visibility[@]}"
        ;;
    *)
        echo "unknown step: ${step}" >&2
        exit 2
        ;;
esac
