#!/usr/bin/env bash
# task version-info: the version is written down three times by hand, and nothing made them agree:
#   Cargo.toml                            [workspace.package] version   (every Rust binary)
#   apps/desktop/src-tauri/tauri.conf.json  "version"                   (the app bundle)
#   apps/desktop/package.json               "version"                   (the frontend)
# A release that bumps one and forgets another ships an app whose About says one thing and whose
# daemon says another, which the app then reports as a build mismatch. Exits 1 and names each.
#
# Usage: scripts/check-version-sync.sh
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

# The first `version = "..."` after [workspace.package]. awk ranges:
# https://www.gnu.org/software/gawk/manual/html_node/Ranges.html
cargo=$(awk '/^\[workspace\.package\]/{p=1;next} /^\[/{p=0} p && /^version[[:space:]]*=/{gsub(/[" ]/,"",$0); sub(/^version=/,"",$0); print; exit}' Cargo.toml)
# node is already a dependency of the desktop build; JSON.parse beats grepping JSON.
# https://nodejs.org/api/cli.html#-p---print-script
tauri=$(node -p 'JSON.parse(require("fs").readFileSync("apps/desktop/src-tauri/tauri.conf.json")).version')
npm=$(node -p 'JSON.parse(require("fs").readFileSync("apps/desktop/package.json")).version')

if [ -z "$cargo" ]; then
  echo "check-version-sync: no [workspace.package] version found in Cargo.toml"
  exit 1
fi
status=0
for pair in "apps/desktop/src-tauri/tauri.conf.json=$tauri" "apps/desktop/package.json=$npm"; do
  file=${pair%%=*}
  version=${pair#*=}
  if [ "$version" != "$cargo" ]; then
    echo "check-version-sync: $file says $version, Cargo.toml says $cargo"
    status=1
  fi
done
[ "$status" -eq 0 ] && echo "check-version-sync: $cargo everywhere"
exit $status
