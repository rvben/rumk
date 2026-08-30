#!/usr/bin/env bash
set -euo pipefail

expected_version="${1:-}"
expected_version="${expected_version#v}"
package_id="$(cargo pkgid --locked)"
version="${package_id##*[#@]}"

if [[ -n "${expected_version}" && "${version}" != "${expected_version}" ]]; then
    echo "release version mismatch: tag=${expected_version} Cargo.toml=${version}" >&2
    exit 1
fi

if ! grep -Fq "## [${version}]" CHANGELOG.md; then
    echo "CHANGELOG.md has no section for ${version}" >&2
    exit 1
fi

package_args=(--locked)
if [[ "${ALLOW_DIRTY:-0}" == "1" ]]; then
    package_args+=(--allow-dirty)
fi

package_files="$(cargo package --list "${package_args[@]}")"
for private_path in PRD.md .github docs examples; do
    if grep -Eq "^${private_path}(/|$)" <<<"${package_files}"; then
        echo "private or development-only path entered the crate package: ${private_path}" >&2
        exit 1
    fi
done

cargo publish --dry-run "${package_args[@]}"
printf '%s\n' "release metadata and crate package validated for ${version}"
