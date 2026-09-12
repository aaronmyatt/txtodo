#!/usr/bin/env bash
# Tier-3 check: every spec mirrored from txtodo-design.md/txtodo-implementation-plan.md must equal
# its source verbatim (plan §5: specs are normative and change in the same PR). Exits 1 and shows
# the diff on drift. Two pairs so far, same shape: add a third the same way rather than a new script.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"

plan=$(awk '/^### 3.2 The `ref:` directory convention/{on=1;next} on&&/^### /{exit} on&&/^[0-9]+\. /{print}' "$ROOT/txtodo-implementation-plan.md")
spec=$(awk '/^## Rules/{on=1;next} on&&/^## /{exit} on&&/^[0-9]+\. /{print}' "$ROOT/specs/ref-directories.md")
if [ "$(printf '%s' "$plan" | wc -l)" -lt 11 ]; then echo "specs-mirror: could not find plan §3.2 rules"; exit 1; fi
if ! diff <(printf '%s\n' "$plan") <(printf '%s\n' "$spec") >/tmp/specs-mirror-ref-directories.diff; then
  echo "specs-mirror: specs/ref-directories.md drifted from plan §3.2:"; cat /tmp/specs-mirror-ref-directories.diff; exit 1
fi

design_conflicts=$(awk '/^### 4.7 Conflict semantics/{on=1;next} on&&/^### /{exit} on&&/^\| /{print}' "$ROOT/txtodo-design.md")
spec_conflicts=$(awk '/^## Rows/{on=1;next} on&&/^## /{exit} on&&/^\| /{print}' "$ROOT/specs/conflicts.md")
if [ "$(printf '%s' "$design_conflicts" | wc -l)" -lt 10 ]; then echo "specs-mirror: could not find design §4.7 table"; exit 1; fi
if ! diff <(printf '%s\n' "$design_conflicts") <(printf '%s\n' "$spec_conflicts") >/tmp/specs-mirror-conflicts.diff; then
  echo "specs-mirror: specs/conflicts.md drifted from design §4.7:"; cat /tmp/specs-mirror-conflicts.diff; exit 1
fi

exit 0
