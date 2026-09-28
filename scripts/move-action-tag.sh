#!/usr/bin/env bash
# Point the moving major Action tag (v0 for 0.x releases) at a published
# release, then read the tag back. The tag only moves forward: a release that
# does not descend from the current tag, such as a backport, leaves it alone.
#
# Usage: move-action-tag.sh VERSION [--dry-run]
# Needs an authenticated gh and GITHUB_REPOSITORY (owner/name).
set -euo pipefail

version="${1:?usage: move-action-tag.sh VERSION [--dry-run]}"
version="${version#v}"
dry_run="${2:-}"
repository="${GITHUB_REPOSITORY:?set GITHUB_REPOSITORY to owner/name}"

if [[ "${version}" == *-* ]]; then
    echo "v${version} is a prerelease; the major Action tag stays where it is"
    exit 0
fi
major="v${version%%.*}"
commit="$(gh api "repos/${repository}/commits/v${version}" --jq .sha)"

if gh api "repos/${repository}/git/ref/tags/${major}" >/dev/null 2>&1; then
    relation="$(gh api "repos/${repository}/compare/${major}...${commit}" --jq .status)"
    case "${relation}" in
        identical)
            echo "${major} already points at v${version}"
            exit 0
            ;;
        ahead) ;;
        *)
            echo "v${version} is ${relation} relative to ${major}; the tag stays where it is"
            exit 0
            ;;
    esac
    request=(-X PATCH "repos/${repository}/git/refs/tags/${major}" -f "sha=${commit}" -F force=true)
else
    request=(-X POST "repos/${repository}/git/refs" -f "ref=refs/tags/${major}" -f "sha=${commit}")
fi

if [[ "${dry_run}" == "--dry-run" ]]; then
    echo "would point ${major} at v${version} (${commit})"
    exit 0
fi
gh api "${request[@]}" >/dev/null

actual="$(gh api "repos/${repository}/commits/${major}" --jq .sha)"
if [[ "${actual}" != "${commit}" ]]; then
    echo "${major} points at ${actual}, expected ${commit}" >&2
    exit 1
fi
echo "${major} now points at v${version} (${commit})"
