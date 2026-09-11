# Maestro UI test mirroring the M7 Playwright scenario with LAN ≤ 2 s and relay ≤ 30 s convergence (plan M9, plan §7)

Plan M9 acceptance: "Shared UI test script (Maestro or equivalent) runs the same scenario as M7's
Playwright suite. A phone and a desktop pair and converge on LAN in ≤ 2 s and via relay in ≤ 30 s."
Maestro drives the real apps on emulator/simulator: https://maestro.mobile.dev/

## One scenario, three platforms

Port the M7 Playwright scenario into Maestro YAML flows, then run the same flow against the
Android app and the iOS app. The scenario asserts the thin-client contract from design §7: the
app renders `tokenize` spans and never re-parses the file, so the assertions check rendered
attributed text, not a private parser.

- Single tap opens the edit popover with the raw line including `id:`; save changes only that line.
- Double tap opens detail; first keystroke into empty `notes.md` creates the directory.
- Conflict banner + review sheet when a second daemon injects concurrent ops.
- `id:` hidden by default; completed line struck through.

## Convergence timing

Pair a phone with a desktop daemon over LAN, then over relay. Time from op applied on one device
to the line appearing on the other.

- LAN ≤ 2 s, relay ≤ 30 s — measured in CI as hard budgets (fail the run, not a warning).
- Relay leg uses the M8 two-network setup (network namespaces) so the relay path is genuinely
  exercised, not loopback.

## Acceptance

The same Maestro flow passes on Android and iOS; the convergence numbers land under both budgets;
a deliberate op from a second daemon produces the conflict sheet.
