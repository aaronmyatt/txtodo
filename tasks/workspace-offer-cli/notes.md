# workspace-offer-cli

## Goal

Give the offer/accept workspace-identity-agreement protocol a CLI (and TUI) surface. The daemon
RPCs already exist; nothing drives them today.

## Why

Found while answering a user question about cross-device workspace sync. `WorkspacePendingOffers`
/ `WorkspaceAcceptOffer` / `WorkspaceDeclineOffer` are implemented at
`crates/txtodo-daemon/src/workspace_offer_grpc.rs`, built as part of the offer/accept identity
agreement (see `tasks/daemon-workspace-identity-agreement/notes.md`,
`tasks/pairing-workspace-identity/notes.md`), but there is no `txtodo workspace ...` subcommand
that lists or resolves a pending offer — a paired device that receives an offered (non-default)
workspace has no way to accept it today outside of calling the RPC directly.

## Design

- Follows the existing `workspace` subcommand shape (`list`/`add`/`remove` at
  `crates/txtodo-cli/src/cli.rs`) — add `offers`, `accept <id> [--dir <path>]`, `decline <id>`
  alongside them.
- `accept` without `--dir` needs a default location — this is where `remote-workspace-mirror`
  (see that ref) picks up: right now the daemon requires the caller to name a `local_dir`
  (`WorkspaceAcceptOfferRequest.local_dir`, `txtodo.proto:790-794`); a bare `accept` with no `--dir`
  is out of scope for this line unless the mirror-default work lands first.

## Dependency

Blocks part of `ref:remote-workspace-mirror`'s CLI/TUI surfacing sub-task — that line needs
`accept` to exist before it can add a "remote" label to what it lists.

## As built (2026-09-23)

- CLI: `txtodo workspace offers|accept <id> --dir <path> [--from <device>]|decline <id> [--from]`
  in `crates/txtodo-cli/src/commands/workspace_offers.rs`, RPC wrappers in `client_workspace.rs`.
  An offer is keyed by (device, workspace id); the CLI takes the workspace id and looks the device
  up among the pending offers, so `--from` is only needed when two peers offer the same id.
  `--dir` is required: the daemon has no default location for a bare accept until
  `remote-workspace-mirror` decides one (that ref's line 4 relaxes this).
- TUI: `o` opens the offers pane (`ui/offers.rs`, `state_offers.rs`, `app_offers.rs`); `a` opens
  a one-line directory prompt and `Enter` accepts into it, `d` declines. The list refreshes on the
  same 1 s tick as the `s` indicator; the status line shows `N workspace offer(s): o` when N > 0.
- Tests: CLI e2e (`tests/workspace_offers.rs`, shared harness in `tests/support/global_daemon.rs`)
  covers the empty list and unknown-offer refusals against a real global daemon. The populated
  path needs a paired peer; that stays in `workspace_offer_grpc_tests.rs` (daemon, in-process).
- Known gaps: no populated-offer test on either client; the TUI shows an accept/decline RPC error
  only in the log (no banner) — same as every other `perform` error today; `workspace.rs` and
  `layout.rs` still carry their own inline copies of the daemon harness (optional tidy-up).
