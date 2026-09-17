#!/usr/bin/env bash
# Tier-3 slice fence (non-interactive form): each crate's [dependencies]/[dev-dependencies] may
# name only the workspace crates listed for it in budgets.json.slices.allowedDeps. Exits 1 on any
# extra edge. Cargo manifest format: https://doc.rust-lang.org/cargo/reference/manifest.html
#
# Also covers apps/desktop/src-tauri (task desktop-stack-gaps): a real root Cargo.toml workspace
# member that this script used to skip entirely, since the loop only ever globbed crates/*. Its
# budgets.json.slices.allowedDeps key is "src-tauri" (basename of its own manifest's directory,
# same derivation the crates/* loop below uses), not "desktop" (its Cargo.toml package name).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
status=0
for manifest in "$ROOT"/crates/*/Cargo.toml "$ROOT"/apps/desktop/src-tauri/Cargo.toml; do
  crate=$(basename "$(dirname "$manifest")")
  # workspace crates referenced in any dependency table of this manifest
  actual=$(awk '/^\[(dev-|build-)?dependencies/{on=1;next} /^\[/{on=0} on && /^txtodo-/{sub(/[ =.].*/,""); print}' "$manifest" | sort -u)
  allowed=$(node -p 'const s=JSON.parse(require("fs").readFileSync(process.argv[1])).slices.allowedDeps;
    (s[process.argv[2]]||[]).sort().join("\n")' "$ROOT/.claude/budgets.json" "$crate")
  extra=$(comm -23 <(echo "$actual" | sed '/^$/d') <(echo "$allowed" | sed '/^$/d') || true)
  if [ -n "$extra" ]; then
    echo "boundary: $crate depends on $(echo "$extra" | tr '\n' ' ')— not in budgets.json allowedDeps"; status=1
  fi
done
exit $status
