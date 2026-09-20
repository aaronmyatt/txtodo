#!/usr/bin/env bash
# deploy/homebrew/update-cask.sh — stamps Casks/txtodo-desktop.rb's version/sha256s for a real
# release tag (task desktop-cask-distribution), the cask's counterpart to update-formula.sh. Two
# args: the tag (e.g. v0.1.0) and a directory already holding that release's 2 desktop assets
# (desktop-macos-{aarch64,x86_64}.dmg — release.yml's own build-desktop job naming). Real usage
# populates that directory first via
# `gh release download "$tag" --pattern '*desktop-macos*' --dir assets/`
# (https://cli.github.com/manual/gh_release_download); this script itself never touches the
# network, same as update-formula.sh — see that script's own doc for why that's what makes it
# testable without a live release at all.
#
# Unlike the formula (no explicit `version`, inferred from each url's own path segment — see
# Formula/txtodo.rb's own comment on why), the cask's `url`s are built from a `version "..."`
# variable (`v#{version}` interpolation) — Homebrew Cask's own convention for a cask with more
# than one url (https://docs.brew.sh/Cask-Cookbook#version) — so this script stamps `version`
# itself, not each url's tag segment.
#
# The sha256-per-asset rewrite uses Python, not sed, unlike update-formula.sh's own "next line
# after the url" trick: `brew style --fix` ordered this cask's stanzas `sha256` *then* `url`
# (Cask/StanzaOrder's own convention, the opposite order the formula's on_arm/on_intel blocks
# use), so the line to rewrite is the one *before* the url match, not after — a plain `sed -n
# '/pat/{n;...}'` one-liner can't express "previous line" without a fragile hold-space dance;
# a few lines of Python are clearer than that here.
set -euo pipefail

TAG="${1:?usage: update-cask.sh <tag> <assets-dir>}"
ASSETS="${2:?usage: update-cask.sh <tag> <assets-dir>}"
HERE="$(cd "$(dirname "$0")" && pwd)"
CASK="${HERE}/Casks/txtodo-desktop.rb"

ASSET_NAMES=(desktop-macos-aarch64.dmg desktop-macos-x86_64.dmg)
for name in "${ASSET_NAMES[@]}"
do
  [[ -f "${ASSETS}/${name}" ]] || {
    echo "update-cask: missing ${ASSETS}/${name}" >&2
    exit 1
  }
done

# `version "0.1.0"`, stripping the tag's leading "v" (bash parameter expansion, a no-op if the tag
# somehow has no "v" prefix): https://www.gnu.org/software/bash/manual/html_node/Shell-Parameter-Expansion.html
version="${TAG#v}"
sed -i '' -E "s/version \"[^\"]+\"/version \"${version}\"/" "${CASK}"

for name in "${ASSET_NAMES[@]}"
do
  sha="$(shasum -a 256 "${ASSETS}/${name}" | awk '{print $1}')"
  ASSET_NAME="${name}" SHA="${sha}" CASK="${CASK}" python3 "${HERE}/stamp-cask-sha.py"
done

echo "update-cask: stamped ${TAG} (version ${version}) into ${CASK}"
