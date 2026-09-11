#!/usr/bin/env bash
# Tier-3 check: the 12 numbered rules in specs/ref-directories.md must equal plan §3.2 verbatim
# (plan §5: specs are normative and change in the same PR). Exits 1 and shows the diff on drift.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
plan=$(awk '/^### 3.2 The `ref:` directory convention/{on=1;next} on&&/^### /{exit} on&&/^[0-9]+\. /{print}' "$ROOT/txtodo-implementation-plan.md")
spec=$(awk '/^## Rules/{on=1;next} on&&/^## /{exit} on&&/^[0-9]+\. /{print}' "$ROOT/specs/ref-directories.md")
if [ "$(printf '%s' "$plan" | wc -l)" -lt 11 ]; then echo "specs-mirror: could not find plan §3.2 rules"; exit 1; fi
if ! diff <(printf '%s\n' "$plan") <(printf '%s\n' "$spec") >/tmp/specs-mirror.diff; then
  echo "specs-mirror: specs/ref-directories.md drifted from plan §3.2:"; cat /tmp/specs-mirror.diff; exit 1
fi
exit 0
