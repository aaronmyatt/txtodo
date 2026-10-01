#!/usr/bin/env bash
# Tier-3 slice fence (non-interactive form): each crate's [dependencies]/[dev-dependencies] may
# name only the workspace crates listed for it in budgets.json.slices.allowedDeps. Exits 1 on any
# extra edge. Cargo manifest format: https://doc.rust-lang.org/cargo/reference/manifest.html
#
# Also covers apps/desktop/src-tauri (task desktop-stack-gaps): a real root Cargo.toml workspace
# member that this script used to skip entirely, since the loop only ever globbed crates/*. Its
# budgets.json.slices.allowedDeps key is "src-tauri" (basename of its own manifest's directory,
# same derivation the crates/* loop below uses), not "desktop" (its Cargo.toml package name).
#
# One node process for every manifest (task fast-gate): the old loop started node, awk and comm per
# crate, 3.2 s for 18 manifests. Same table rule as before: a line starting `txtodo-` inside a
# [dependencies], [dev-dependencies] or [build-dependencies] table.
# https://nodejs.org/api/fs.html#fsreaddirsyncpath-options
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
exec node -e '
  const fs = require("fs"), path = require("path");
  const root = process.argv[1];
  const allowed = JSON.parse(fs.readFileSync(path.join(root, ".claude/budgets.json"))).slices.allowedDeps;
  const manifests = fs.readdirSync(path.join(root, "crates"), { withFileTypes: true })
    .filter((d) => d.isDirectory())
    .map((d) => path.join(root, "crates", d.name, "Cargo.toml"))
    .filter((m) => fs.existsSync(m))
    .sort()
    .concat([path.join(root, "apps/desktop/src-tauri/Cargo.toml")]);
  let status = 0;
  for (const manifest of manifests) {
    const crate = path.basename(path.dirname(manifest));
    let on = false;
    const actual = new Set();
    for (const line of fs.readFileSync(manifest, "utf8").split("\n")) {
      if (/^\[(dev-|build-)?dependencies/.test(line)) { on = true; continue; }
      if (/^\[/.test(line)) { on = false; continue; }
      if (on && /^txtodo-/.test(line)) actual.add(line.replace(/[ =.].*/, ""));
    }
    const ok = new Set(allowed[crate] || []);
    const extra = [...actual].filter((d) => !ok.has(d)).sort();
    if (extra.length) {
      console.log(`boundary: ${crate} depends on ${extra.join(" ")} — not in budgets.json allowedDeps`);
      status = 1;
    }
  }
  process.exit(status);
' "$ROOT"
