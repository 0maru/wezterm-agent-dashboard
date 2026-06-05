#!/usr/bin/env bash

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/.." && pwd)"
target_file="${script_dir}/release-targets.txt"
release_workflow="${repo_root}/.github/workflows/release.yml"

normalize_targets() {
    grep -vE '^[[:space:]]*(#|$)' "$1" | sort
}

expected_targets="$(normalize_targets "${target_file}")"
matrix_targets="$(
    awk '/^[[:space:]]*- target: / { print $3 }' "${release_workflow}" | sort
)"

if [[ "${expected_targets}" != "${matrix_targets}" ]]; then
    echo "ERROR: release target list and release workflow matrix differ" >&2
    echo "Expected targets:" >&2
    echo "${expected_targets}" >&2
    echo "Workflow targets:" >&2
    echo "${matrix_targets}" >&2
    exit 1
fi

echo "Release targets are consistent:"
echo "${expected_targets}"
