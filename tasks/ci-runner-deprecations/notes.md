# CI runner and action deprecations

## Goal

Two GitHub-side deprecations surfaced as annotations on every `ci` and `release` run
(seen on run 36107183277 and release run 36109130613, 2026-09-25). Neither is breaking
anything today; one has a hard date.

- **Node 20 → 24.** Four `actions/*@v4` still declare Node 20. GitHub is *already* forcing
  them onto Node 24, so this is noise, not breakage — but the forcing is temporary.
- **`ubuntu-latest` → Ubuntu 26, from 2026-10-19.** This is the one with a date and the one
  that can actually break a build: a new base image changes system packages, and this repo's
  Linux legs install system dependencies and cross-compile with zigbuild.

## Inventory (2026-09-25)

Only two workflows exist: `.github/workflows/ci.yml` and `.github/workflows/release.yml`.

Actions declaring Node 20, with occurrence counts across both files:

| Action | Count | Target |
|---|---|---|
| `actions/checkout@v4` | 15 | v5 |
| `actions/upload-artifact@v4` | 6 | v5 |
| `actions/download-artifact@v4` | 2 | v5 |
| `actions/setup-node@v4` | 2 | v5 |

Not affected (no Node 20 warning): `Swatinem/rust-cache@v2`, `dtolnay/rust-toolchain`,
`taiki-e/install-action`, `sigstore/cosign-installer@v3`, `EmbarkStudios/cargo-deny-action@v2`,
`cachix/install-nix-action@v27`.

`ubuntu-latest` appears 21 times (13 as a bare `runs-on`, 8 inside matrix entries).
`macos-latest` ×4 and `windows-latest` ×1 are unaffected by the Ubuntu migration.

## Design

**`.github/**` is a frozen path** (`.claude/budgets.json` `slices.frozenPaths`), and there is no
`unlockSentinel` configured, so an agent cannot edit these files at all — not even with a human
saying yes in chat. The repo's own precedent for this is a patch file a human pastes in:
`RELEASE_CI.patch.md`, `RELEASE_CONVERGE_CI.patch.md`. Do the same here rather than trying to
route around the fence.

So the deliverable is a patch file plus a verified-after check, not a commit to the workflows.

## Open question (`@human`)

Pin the Linux legs to `ubuntu-24.04`, or ride `ubuntu-latest` into Ubuntu 26?

- **Pin**: CI stops changing under us; the upgrade becomes a deliberate, separate commit whose
  failures are attributable. Cost: the pin rots, and `ubuntu-24.04` eventually goes away too.
- **Ride**: no maintenance, but the first red run after 19 October could be the migration rather
  than a real regression, and this repo's Linux legs are the ones that install system deps and
  cross-compile — the most likely place for an image change to bite.

I'd pin, then bump deliberately: this repo already has enough intermittently-red CI that a
self-inflicted ambiguous failure is worth avoiding. Not an agent's call either way.

## As built

- Nothing yet.
