#!/usr/bin/env bash
set -euo pipefail

version="${GHA_RUMK_VERSION:-}"
command_name="${GHA_RUMK_COMMAND:-check}"
report_type="${GHA_RUMK_REPORT_TYPE:-logs}"
fail_on_error="${GHA_RUMK_FAIL_ON_ERROR:-true}"
install_only="${GHA_RUMK_INSTALL_ONLY:-false}"

if [[ ! "${version}" =~ ^[0-9]+\.[0-9]+\.[0-9]+([+-][0-9A-Za-z.-]+)?$ ]]; then
    echo "::error::Invalid Rumk version '${version}'; expected an exact SemVer version"
    exit 2
fi

case "${command_name}" in
    check | fmt-check | fmt) ;;
    *)
        echo "::error::Invalid command '${command_name}'; expected check, fmt-check, or fmt"
        exit 2
        ;;
esac

case "${report_type}" in
    logs | annotations) ;;
    *)
        echo "::error::Invalid report-type '${report_type}'; expected logs or annotations"
        exit 2
        ;;
esac

for boolean_input in "fail-on-error:${fail_on_error}" "install-only:${install_only}"; do
    input_name="${boolean_input%%:*}"
    input_value="${boolean_input#*:}"
    if [[ "${input_value}" != "true" && "${input_value}" != "false" ]]; then
        echo "::error::Invalid ${input_name} value '${input_value}'; expected true or false"
        exit 2
    fi
done

if [[ "${command_name}" == "fmt-check" && "${report_type}" == "annotations" ]]; then
    echo "::error::fmt-check requires report-type logs because formatting diffs are text-only"
    exit 2
fi

resolve_target() {
    local os_name="${RUNNER_OS:-}" arch_name="${RUNNER_ARCH:-}"
    local platform_os="" platform_arch=""

    case "${os_name}" in
        Linux) platform_os="linux" ;;
        macOS) platform_os="macos" ;;
        Windows) platform_os="windows" ;;
        *)
            case "$(uname -s)" in
                Linux*) platform_os="linux" ;;
                Darwin*) platform_os="macos" ;;
                MINGW* | MSYS* | CYGWIN*) platform_os="windows" ;;
            esac
            ;;
    esac

    case "${arch_name}" in
        X64 | x86_64 | AMD64) platform_arch="x86_64" ;;
        ARM64 | arm64 | aarch64) platform_arch="aarch64" ;;
        *)
            case "$(uname -m)" in
                x86_64 | amd64) platform_arch="x86_64" ;;
                aarch64 | arm64) platform_arch="aarch64" ;;
            esac
            ;;
    esac

    case "${platform_os}-${platform_arch}" in
        linux-x86_64) printf '%s\n' "x86_64-unknown-linux-gnu tar.gz" ;;
        linux-aarch64) printf '%s\n' "aarch64-unknown-linux-gnu tar.gz" ;;
        macos-x86_64) printf '%s\n' "x86_64-apple-darwin tar.gz" ;;
        macos-aarch64) printf '%s\n' "aarch64-apple-darwin tar.gz" ;;
        windows-x86_64) printf '%s\n' "x86_64-pc-windows-msvc zip" ;;
    esac
}

sha256_of() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{print $1}'
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$1" | awk '{print $1}'
    else
        echo "::error::No SHA-256 implementation is available"
        return 1
    fi
}

download() {
    local url="$1" destination="$2" http_code="" curl_status=0
    http_code="$(curl --location --silent --show-error --retry 3 --retry-all-errors \
        --user-agent "rumk-action" --output "${destination}" --write-out '%{http_code}' \
        "${url}")" || curl_status=$?
    if [[ "${curl_status}" -ne 0 ]]; then
        echo "::error::Download failed for ${url} (curl exit ${curl_status})"
        return 1
    fi
    if [[ "${http_code}" != "200" ]]; then
        echo "::error::Download failed for ${url} (HTTP ${http_code})"
        return 1
    fi
}

resolve_bin_dir() {
    local directory=""
    if [[ -n "${RUNNER_TEMP:-}" && -d "${RUNNER_TEMP}" ]]; then
        directory="${RUNNER_TEMP}/rumk-bin"
    else
        directory="$(mktemp -d)/rumk-bin"
    fi
    mkdir -p "${directory}"
    chmod 700 "${directory}"
    printf '%s\n' "${directory}"
}

target_info="$(resolve_target)"
if [[ -z "${target_info}" ]]; then
    echo "::error::Rumk has no release binary for RUNNER_OS='${RUNNER_OS:-}' RUNNER_ARCH='${RUNNER_ARCH:-}'"
    exit 2
fi
if ! command -v curl >/dev/null 2>&1; then
    echo "::error::curl is required to install Rumk"
    exit 2
fi

read -r target archive_format <<<"${target_info}"
tag="v${version}"
package="rumk-${version}-${target}"
asset="${package}.${archive_format}"
base_url="https://github.com/rvben/rumk/releases/download/${tag}"
workdir="$(mktemp -d)"
trap 'rm -rf "${workdir}"' EXIT

echo "Installing Rumk ${version} for ${target}"
download "${base_url}/${asset}" "${workdir}/${asset}"
download "${base_url}/SHA256SUMS" "${workdir}/SHA256SUMS"

expected_hash="$(awk -v asset="${asset}" '$2 == asset || $2 == ("*" asset) { print $1; exit }' "${workdir}/SHA256SUMS")"
if [[ ! "${expected_hash}" =~ ^[[:xdigit:]]{64}$ ]]; then
    echo "::error::SHA256SUMS has no valid entry for ${asset}"
    exit 1
fi
actual_hash="$(sha256_of "${workdir}/${asset}")"
expected_hash="$(printf '%s' "${expected_hash}" | tr '[:upper:]' '[:lower:]')"
actual_hash="$(printf '%s' "${actual_hash}" | tr '[:upper:]' '[:lower:]')"
if [[ "${expected_hash}" != "${actual_hash}" ]]; then
    echo "::error::Checksum mismatch for ${asset}"
    exit 1
fi
echo "Verified ${asset} against the release checksum manifest"

if [[ "${archive_format}" == "zip" ]]; then
    if [[ -x /c/Windows/System32/tar.exe ]]; then
        /c/Windows/System32/tar.exe -xf "${workdir}/${asset}" -C "${workdir}"
    elif command -v powershell >/dev/null 2>&1; then
        powershell -NoProfile -Command \
            "Expand-Archive -LiteralPath '${workdir}/${asset}' -DestinationPath '${workdir}' -Force"
    else
        echo "::error::No ZIP extractor is available"
        exit 2
    fi
    binary_name="rumk.exe"
else
    tar -xzf "${workdir}/${asset}" -C "${workdir}"
    binary_name="rumk"
fi

source_binary="${workdir}/${package}/${binary_name}"
if [[ ! -f "${source_binary}" ]]; then
    echo "::error::Release archive does not contain ${package}/${binary_name}"
    exit 1
fi

bin_dir="$(resolve_bin_dir)"
rumk_cmd="${bin_dir}/${binary_name}"
mv "${source_binary}" "${rumk_cmd}"
chmod +x "${rumk_cmd}"
if [[ -n "${GITHUB_PATH:-}" ]]; then
    printf '%s\n' "${bin_dir}" >>"${GITHUB_PATH}"
fi

version_output="$("${rumk_cmd}" version)"
installed_version="${version_output#rumk }"
if [[ "${installed_version}" != "${version}" ]]; then
    echo "::error::Installed Rumk version '${installed_version}' does not match requested version '${version}'"
    exit 1
fi
echo "Installed: ${version_output}"

if [[ -n "${GITHUB_OUTPUT:-}" ]]; then
    printf 'rumk-version=%s\n' "${installed_version}" >>"${GITHUB_OUTPUT}"
    printf 'rumk-path=%s\n' "${rumk_cmd}" >>"${GITHUB_OUTPUT}"
fi

if [[ "${install_only}" == "true" ]]; then
    echo "install-only: Rumk is available on PATH for subsequent steps"
    exit 0
fi

cli_args=(--color never)
if [[ -n "${GHA_RUMK_CONFIG:-}" ]]; then
    config_path="${GHA_RUMK_CONFIG}"
    if [[ "${config_path}" != /* && ! "${config_path}" =~ ^[A-Za-z]:[\\/] ]]; then
        config_path="$(pwd)/${config_path}"
    fi
    cli_args+=(--config "${config_path}")
fi

case "${command_name}" in
    check)
        cli_args+=(check)
        echo "Linting Makefiles with Rumk"
        ;;
    fmt-check)
        cli_args+=(fmt --check)
        echo "Checking Makefile formatting with Rumk"
        ;;
    fmt)
        cli_args+=(fmt)
        echo "Formatting Makefiles with Rumk; files may be rewritten"
        ;;
esac

if [[ "${report_type}" == "annotations" ]]; then
    cli_args+=(--output-format github)
fi

if [[ -n "${GHA_RUMK_ARGS:-}" ]]; then
    read -r -a extra_args <<<"${GHA_RUMK_ARGS}"
    cli_args+=("${extra_args[@]}")
fi

read -r -a target_paths <<<"${GHA_RUMK_PATH:-.}"
if [[ "${#target_paths[@]}" -eq 0 ]]; then
    target_paths=(.)
fi
echo "Path(s): ${target_paths[*]}"

set +e
results="$("${rumk_cmd}" "${cli_args[@]}" "${target_paths[@]}" 2>&1)"
exit_code=$?
set -e

if [[ -n "${results}" ]]; then
    printf '%s\n' "${results}"
fi

if [[ -n "${GHA_RUMK_OUTPUT_FILE:-}" ]]; then
    output_file="${GHA_RUMK_OUTPUT_FILE}"
    output_directory="$(dirname "${output_file}")"
    if [[ "${output_directory}" != "." ]]; then
        mkdir -p "${output_directory}"
    fi
    printf '%s\n' "${results}" >"${output_file}"
    echo "Results written to ${output_file}"
fi

if [[ "${exit_code}" -eq 0 ]]; then
    exit 0
fi
if [[ "${exit_code}" -ne 1 ]]; then
    echo "::error::Rumk failed with tool exit code ${exit_code}"
    exit "${exit_code}"
fi
if [[ "${fail_on_error}" == "true" ]]; then
    exit 1
fi

echo "::notice::Rumk found violations (informational mode)"
exit 0
