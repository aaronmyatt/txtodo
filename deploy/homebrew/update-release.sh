#!/usr/bin/env bash
# deploy/homebrew/update-release.sh — one release tag, one pass: stamps both Formula/txtodo.rb
# (update-formula.sh) and Casks/txtodo-desktop.rb (update-cask.sh) from the same assets directory,
# so the two never drift by being run separately (task desktop-cask-distribution's own todo.txt:
# "one release-bump script rewrites both... together"). Same two-arg shape as each script it
# wraps: a tag and a directory already holding every macOS release asset (the 6 CLI ones
# update-formula.sh needs plus the 2 desktop ones update-cask.sh needs) —
# `gh release download "$tag" --pattern '*macos*' --dir assets/` gets all 8 in one call, since
# every one of them carries "macos" in its name.
set -euo pipefail

TAG="${1:?usage: update-release.sh <tag> <assets-dir>}"
ASSETS="${2:?usage: update-release.sh <tag> <assets-dir>}"
HERE="$(cd "$(dirname "$0")" && pwd)"

"$HERE/update-formula.sh" "$TAG" "$ASSETS"
"$HERE/update-cask.sh" "$TAG" "$ASSETS"
