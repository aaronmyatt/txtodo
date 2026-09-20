#!/usr/bin/env bash
# One-off, this machine only (task workspace-layout, decided 2026-09-20): the default place for a
# root list's ref folders flips from beside the list (<root>/<slug>, ADR 0012) to <root>/tasks/<slug>.
# The project is young and no other device syncs these workspaces yet, so there is no pin-at-first-
# open and no `txtodo refs migrate` in the product: this script moves what is on this disk, once.
#
# For every active workspace in the registry whose root still exists, and for every `ref:<slug>` in
# its root todo.txt: when <root>/<slug> is a directory and <root>/tasks/<slug> is not there yet, it
# is moved to <root>/tasks/<slug>. Nested lists are left alone: a line inside tasks/<slug>/todo.txt
# still resolves beside its own file. A slug that exists in both places is reported and skipped.
#
# Dry run by default; nothing moves without --yes. It refuses to move anything while a txtodod is
# running: the daemon keys a document's history by its path, so a folder moved under a live daemon
# reads as "document deleted" plus "new document". Even with the daemon stopped, a moved sub-list
# starts a fresh history on the next start; the todo.txt and notes.md bytes are untouched.
#
# Usage: scripts/migrate-refs-to-tasks.sh [--yes]
#   TXTODO_REGISTRY_DB overrides the registry path, as it does for txtodod.
set -euo pipefail

apply=0
[ "${1:-}" = "--yes" ] && apply=1

db="${TXTODO_REGISTRY_DB:-${XDG_DATA_HOME:-$HOME/.local/share}/txtodo/registry.db}"
if [ ! -f "$db" ]; then
  echo "migrate-refs: no registry at $db, nothing to do"
  exit 0
fi
# pgrep -x matches the process name exactly. https://man7.org/linux/man-pages/man1/pgrep.1.html
if [ "$apply" -eq 1 ] && pgrep -x txtodod >/dev/null 2>&1; then
  echo "migrate-refs: a txtodod is running. Quit the desktop app, run 'txtodo daemon stop', then retry."
  exit 1
fi

moved=0
conflicts=0
# -readonly: this script never writes the registry. https://www.sqlite.org/cli.html
while IFS= read -r root; do
  [ -d "$root" ] || continue
  list="$root/todo.txt"
  [ -f "$list" ] || continue
  # The slug grammar (specs/todotxt.abnf): lowercase kebab, no slash, no dot, so no path escapes.
  for slug in $(grep -oE '(^| )ref:[a-z0-9][a-z0-9-]{0,39}( |$)' "$list" | sed -E 's/.*ref:([a-z0-9-]+).*/\1/' | sort -u); do
    from="$root/$slug"
    to="$root/tasks/$slug"
    [ -d "$from" ] || continue
    if [ -e "$to" ]; then
      echo "CONFLICT  $from and $to both exist; left alone"
      conflicts=$((conflicts + 1))
      continue
    fi
    if [ "$apply" -eq 1 ]; then
      mkdir -p "$root/tasks"
      mv "$from" "$to"
      echo "moved     $from -> $to"
    else
      echo "would move $from -> $to"
    fi
    moved=$((moved + 1))
  done
done < <(sqlite3 -readonly "$db" "select root from workspaces where removed_at is null")

if [ "$apply" -eq 1 ]; then
  echo "migrate-refs: moved $moved folder(s), $conflicts conflict(s)"
else
  echo "migrate-refs: dry run, $moved folder(s) to move, $conflicts conflict(s); rerun with --yes to apply"
fi
[ "$conflicts" -eq 0 ]
