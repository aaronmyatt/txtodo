#!/usr/bin/env bash
# Tier-3 check: no .rs file under crates/ (or apps/desktop/src-tauri, task desktop-stack-gaps — a
# real Cargo workspace member this script used to skip entirely) may exceed budgets.json.fileLines.
# Generated artifacts are exempt (constitution §6) — the list lives in budgets.json.generatedPaths so
# this script, gate.sh and the Pi twin cannot drift. Exits 1 and lists offenders.
#
# Args (task fast-gate): optional file paths. Given, only those .rs files are checked (the fast gate
# passes the changed ones); none, the whole tree. One `wc -l` over every file and one awk pass: the
# old per-file `echo | grep` + `wc` loop forked ~2,000 times and took 22 s over 756 files.
# https://pubs.opengroup.org/onlinepubs/9799919799/utilities/wc.html
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
B="$ROOT/.claude/budgets.json"
MAX=$(node -p 'JSON.parse(require("fs").readFileSync(process.argv[1])).fileLines' "$B")
# glob → regex, anchored on the repo-relative path; no generatedPaths means match nothing.
EXEMPT=$(node -p '
  const b=JSON.parse(require("fs").readFileSync(process.argv[1]));
  const g=(b.generatedPaths||[]);
  g.length ? g.map(x=>"^"+x.replace(/[.+^${}()|[\]\\]/g,"\\$&").replace(/\*\*/g,"\0").replace(/\*/g,"[^/]*").replace(/\0/g,".*")+"$").join("|") : "^$";
' "$B")
list() {
  if [ $# -eq 0 ]; then
    find "$ROOT/crates" "$ROOT/apps/desktop/src-tauri" -name '*.rs' -not -path '*/target/*' -print0
    return
  fi
  local f abs
  for f in "$@"; do
    case "$f" in /*) abs="$f" ;; *) abs="$ROOT/$f" ;; esac
    case "$abs" in "$ROOT"/crates/*.rs | "$ROOT"/apps/desktop/src-tauri/*.rs) ;; *) continue ;; esac
    case "$abs" in */target/*) continue ;; esac
    if [ -f "$abs" ]; then printf '%s\0' "$abs"; fi
  done
}
# EXEMPT goes in through ENVIRON, not -v: awk would eat the backslashes in `\.` from a -v value.
# `xargs -0 wc -l` prints "<n> <path>" per file, plus a "<n> total" line when given more than one.
list "$@" | xargs -0 wc -l | EXEMPT="$EXEMPT" awk -v max="$MAX" -v root="$ROOT/" '
  { n=$1; sub(/^[ \t]*[0-9]+[ \t]+/, ""); path=$0 }
  path == "total" { next }
  {
    rel = substr(path, 1, length(root)) == root ? substr(path, length(root) + 1) : path
    if (rel ~ ENVIRON["EXEMPT"]) next
    if (n + 0 > max + 0) {
      printf "file-length: %s has %d lines (max %d) — split the file; move long rationale comments to the task'"'"'s tasks/<slug>/notes.md (leave a one-line pointer)\n", rel, n, max
      bad = 1
    }
  }
  END { exit bad }'
