#!/usr/bin/env bash
# wezterm-agent-dashboard の Homebrew formula をその場で更新する。
#
# 使い方:
#   scripts/update-formula.sh <formula-path> <tag> <checksums-file>
#
# 例:
#   scripts/update-formula.sh tap/Formula/wezterm-agent-dashboard.rb v0.2.0 /tmp/checksums.txt
#
# checksums-file 形式（release workflow の `sha256sum` 出力）:
#   <sha256>  wezterm-agent-dashboard-v0.2.0-aarch64-apple-darwin.tar.gz
#   <sha256>  wezterm-agent-dashboard-v0.2.0-x86_64-apple-darwin.tar.gz
#   <sha256>  wezterm-agent-dashboard-v0.2.0-x86_64-unknown-linux-gnu.tar.gz
#
# このスクリプトで行うこと:
#   1. tag 先頭の 'v' を外して version を得る
#   2. scripts/release-targets.txt の各 target について sha256 を探す
#   3. formula の `version "..."` と対象 URL 直後の `sha256 "..."` を置換する
#   4. 置換後の formula に新しい sha256 が含まれることを検証する
#
# BSD/GNU sed の差を避けるため、-i.bak を使ってから .bak を削除する。

set -euo pipefail

if [[ $# -ne 3 ]]; then
    echo "Usage: $0 <formula-path> <tag> <checksums-file>" >&2
    exit 1
fi

formula="$1"
tag="$2"
checksums="$3"
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
targets_file="${script_dir}/release-targets.txt"

if [[ ! -f "$formula" ]]; then
    echo "ERROR: formula not found: $formula" >&2
    exit 1
fi
if [[ ! -f "$checksums" ]]; then
    echo "ERROR: checksums file not found: $checksums" >&2
    exit 1
fi
if [[ ! -f "$targets_file" ]]; then
    echo "ERROR: release target list not found: $targets_file" >&2
    exit 1
fi

# v0.2.0 -> 0.2.0
version="${tag#v}"

# checksums.txt から指定 target の sha256 行を取り出す。
# target が存在しない場合は明示的に失敗させる。
get_sha() {
    local target="$1"
    local pattern="wezterm-agent-dashboard-${tag}-${target}.tar.gz"
    local sha
    sha=$(awk -v p="$pattern" '$2 == p { print $1 }' "$checksums")
    if [[ -z "$sha" ]]; then
        echo "ERROR: sha256 for ${target} not found in ${checksums}" >&2
        echo "       expected filename: ${pattern}" >&2
        exit 1
    fi
    echo "$sha"
}

targets=()
while IFS= read -r target; do
    [[ -z "$target" || "$target" =~ ^[[:space:]]*# ]] && continue
    targets+=("$target")
done < "$targets_file"

shas=()
for target in "${targets[@]}"; do
    shas+=("$(get_sha "$target")")
done

# BSD/GNU sed 両対応のため .bak suffix を使う。
sed_inplace() {
    sed -i.bak "$@"
}

# version 宣言を更新する。
sed_inplace "s|version \"[^\"]*\"|version \"${version}\"|" "$formula"

# target 名を含む URL 行に一致させ、直後の sha256 行だけを更新する。
# {n;...;} で URL 行の次の行へ進めてから sha256 を置換する。
for idx in "${!targets[@]}"; do
    target="${targets[$idx]}"
    sha="${shas[$idx]}"
    sed_inplace "/${target}\.tar\.gz/{n;s|sha256 \"[^\"]*\"|sha256 \"${sha}\"|;}" "$formula"
done

rm -f "${formula}.bak"

# 更新後の formula にすべての新しい sha256 が含まれることを確認する。
for sha in "${shas[@]}"; do
    if ! grep -q "$sha" "$formula"; then
        echo "ERROR: sha256 ${sha} not found in formula after update" >&2
        echo "       (sed pattern may have failed to match)" >&2
        exit 1
    fi
done

echo "Updated ${formula} -> version ${version}"
for idx in "${!targets[@]}"; do
    printf '  %s: %s\n' "${targets[$idx]}" "${shas[$idx]}"
done
