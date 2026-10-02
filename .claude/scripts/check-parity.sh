#!/usr/bin/env bash
# Advisory check (ADR 0031, tasks/tui-revamp/parity-manifest): warns when a client's keys or screens
# change and specs/client-parity.toml does not, since the manifest must change in the same commit as
# any user-facing action. Never fails: it prints a warning and exits 0 (human decide line, 2026-09-25:
# "advisory only, never blocking the gate"). The gate shows the warning; the tests are the hard check
# (crates/txtodo-tui/tests/it/parity.rs, apps/desktop/src/lib/keys.parity.test.ts).
# Changed = tracked files that differ from HEAD, plus untracked ones.
# Ref: https://git-scm.com/docs/git-diff#Documentation/git-diff.txt---name-only
set -uo pipefail
cd "$(dirname "$0")/../.." || exit 0
MANIFEST=specs/client-parity.toml
# What a user can press or see: the TUI keymap, input and drawing code; desktop's keys.ts. Tests
# are left out: they change without changing what a user does.
WATCHED='^(crates/txtodo-tui/src/(keymap\.rs|keymap/.*|input[a-z_]*\.rs|ui/.*)|apps/desktop/src/lib/keys\.ts)$'
changed=$( { git diff HEAD --name-only 2>/dev/null; git ls-files --others --exclude-standard 2>/dev/null; } | sort -u)
grep -qx "$MANIFEST" <<<"$changed" && exit 0
hits=$(grep -E "$WATCHED" <<<"$changed" | grep -Ev '_tests\.rs$' || true)
[ -z "$hits" ] && exit 0
echo "parity (advisory): these changed but $MANIFEST did not:"
sed 's/^/  /' <<<"$hits"
echo "If a key or a user-facing action changed, update the manifest in the same commit (ADR 0031)."
exit 0
