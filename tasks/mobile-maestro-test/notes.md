# Maestro UI test mirroring the M7 Playwright scenario with LAN ≤ 2 s and relay ≤ 30 s convergence (plan M9, plan §7)

## Goal

Plan M9 acceptance: "Shared UI test script (Maestro or equivalent) runs the same scenario as M7's
Playwright suite. A phone and a desktop pair and converge on LAN in ≤ 2 s and via relay in ≤ 30 s."
One Maestro flow drives the real Android and iOS apps through the M7 interaction scenario, and two
convergence tests enforce the LAN/relay budgets as hard CI failures.

## Design

Maestro drives real apps on emulator/simulator with YAML flows:
https://maestro.mobile.dev/. The scenario is a direct port of M7's Playwright cases (plan M7
acceptance), adapted from click→tap and `id:`→asserted raw text.

```yaml
# e2e/maestro/m7-scenario.yaml — shared by Android (appId dev.txtodo.app) and iOS (dev.txtodo.Txtodo)
appId: ${APP_ID}
---
# 1. Single tap opens the edit popover with the RAW line including id:
- tapOn: "Draft Q4 roadmap"
- assertVisible: "id:.*"              # the edit field shows the raw line, id: included
- tapOn: "Save"
- assertVisible: "Draft Q4 roadmap"   # only that line changed
# 2. id: hidden by default, completed line struck through (rendered tokenize spans, not re-parse)
- assertNotVisible: "id:.*"           # main list hides id:
- assertVisible: "x .*Roadmap"        # completion marker + muted/struck line
# 3. Double tap opens detail; first keystroke into empty notes.md creates the directory
- doubleTapOn: "Draft Q4 roadmap"
- tapOn: "notes.md"
- inputText: "First note"
- assertVisible: "First note"
```

- Conflict banner + review sheet: a second daemon injects concurrent ops (as in M7's
  `needs_review` injection) and the flow asserts the review sheet with the three M4 variants
  (keep mine / keep theirs / keep merged).
- Sub-list: double-tapping a sub-list line nests the breadcrumb (`todo.txt › 2 ›
  q4-roadmap/todo.txt › 3`).
- The assertions check rendered *attributed text* (tokenize spans), never a private parser — the
  thin-client contract from design §7.

### Convergence timing

Pair a phone with a desktop daemon over LAN, then over relay. Time from op applied on one device to
the line appearing on the other.

- LAN ≤ 2 s, relay ≤ 30 s — measured in CI as hard budgets (fail the run, not a warning); these are
  the design §10 SLOs.
- Relay leg uses the M8 two-network setup (network namespaces / two-VM) so the relay path is
  genuinely exercised, never loopback — same harness as M8's relay acceptance.

## Placement/dependencies

- `e2e/maestro/` (new, non-frozen) holds the flows and the driver script; CI workflow (`.github/`,
  frozen) gains a Maestro job — that write is **asked, never silent**.
- Depends on `mobile-pairing-widgets` (QR pairing to establish the LAN/relay pair), M8
  `relay-internet-push` harness (two-network simulation), and the M7 Playwright suite for the exact
  scenario to mirror. No Rust crates change.

## Edge cases & invariants

- Timing budgets are wall-clock assertions on real devices; the CI job must pin the emulator/simulator
  and retry on infra flake (explicit, bounded retries — never a silent skip or a loosened budget).
- The relay leg must route through the relay (assert the peer's transport is `relay`, not LAN), or a
  silent LAN fallback makes the ≤ 30 s number meaningless.
- Maestro selectors must not depend on the hidden `id:` text (it's filtered); use description text
  and the assertion IDs above.
- The conflict-sheet flow injects ops via a *second daemon* (a real concurrent writer), not a mock.

## Acceptance

- The same Maestro flow passes on Android and iOS.
- LAN convergence ≤ 2 s and relay convergence ≤ 30 s both pass as hard CI budgets.
- A deliberate concurrent op from a second daemon produces the conflict sheet, asserted on both
  platforms.

## References

- plan M9 acceptance + M7 acceptance (txtodo-implementation-plan.md); design §7 thin-client + §10 SLOs
- https://maestro.mobile.dev/
