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
