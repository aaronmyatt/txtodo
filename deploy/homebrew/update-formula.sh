#!/usr/bin/env bash
# deploy/homebrew/update-formula.sh — stamps Formula/txtodo.rb's urls/sha256s for a real release
# tag (task brew-distribution). Two args: the tag (e.g. v0.1.0) and a directory already holding
# that release's 6 macOS assets (txtodo{,d,-tui}-macos-{aarch64,x86_64} — RELEASE_CI.patch.md's own
# naming). Real usage populates that directory first via
# `gh release download "$tag" --pattern '*macos*' --dir assets/`
# (https://cli.github.com/manual/gh_release_download); this script itself never touches the
# network, which is what makes it testable without a live release at all — see
# tasks/brew-distribution/notes.md's own "what can be verified now" section.
set -euo pipefail

TAG="${1:?usage: update-formula.sh <tag> <assets-dir>}"
ASSETS="${2:?usage: update-formula.sh <tag> <assets-dir>}"
FORMULA="$(cd "$(dirname "$0")" && pwd)/Formula/txtodo.rb"

ASSET_NAMES=(
  txtodo-macos-aarch64 txtodod-macos-aarch64 txtodo-tui-macos-aarch64
  txtodo-macos-x86_64 txtodod-macos-x86_64 txtodo-tui-macos-x86_64
)
for name in "${ASSET_NAMES[@]}"; do
  [ -f "$ASSETS/$name" ] || { echo "update-formula: missing $ASSETS/$name" >&2; exit 1; }
done

# Every url shares the same .../releases/download/<old-tag>/<name> shape — rewrite the tag segment
# for all of them in one pass. macOS/BSD sed's -i needs an explicit (empty) backup-suffix argument;
# GNU sed accepts the same form, so this stays portable to a Linux CI runner too.
sed -i '' -E "s#(/releases/download/)[^/]+(/)#\1${TAG}\2#g" "$FORMULA"

# Each sha256 line immediately follows its own url line — rewrite by matching the *next* line
# after each asset's own url (anchored on a trailing quote so e.g. "txtodo-macos-aarch64" can never
# false-match inside "txtodo-tui-macos-aarch64"), one asset at a time, so a stale hash can never
# survive attached to the wrong url.
for name in "${ASSET_NAMES[@]}"; do
  sha="$(shasum -a 256 "$ASSETS/$name" | awk '{print $1}')"
  sed -i '' -E "/\/${name}\"/{n; s#sha256 \"[0-9a-f]+\"#sha256 \"${sha}\"#;}" "$FORMULA"
done

echo "update-formula: stamped ${TAG} into $FORMULA"
