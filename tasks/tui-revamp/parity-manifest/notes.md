# parity-manifest

## Goal
- One checked-in list of every user-facing action, with the status of each in both clients.
- Tests fail when a client's keymap and the list disagree.
- Agents record deviations instead of letting them drift silently.

## Design
- `specs/client-parity.toml`:
  - `[[action]]` fields: `id`, `title`, `keys`, `scope`, `desktop = {status, surface}`, `tui = {status, surface}`, `deviation`.
  - `id` doubles as the TUI `:` command name.
  - `keys` use canonical names. `Mod` means Cmd on macOS, Ctrl elsewhere.
  - `status` is one of `done | planned | differs | na`. `differs` and `na` need a one-line `deviation`.
  - `[[screen]]` maps each desktop surface to its TUI surface. The map is in `../notes.md`.
- TUI check: `crates/txtodo-tui/tests/parity.rs`.
  - Parses the manifest with `toml`, a dev-dep only.
  - Every `tui.status = done` id must be in `keymap::BINDINGS` with the same keys, and the reverse.
- Desktop check: `apps/desktop/src/lib/keys.parity.test.ts`, the same checks against `keys.ts`.
  - It is skipped until the revamp creates `keys.ts`.
- Agent rules go in `crates/txtodo-tui/CLAUDE.md` and the desktop guidance:
  - Update the manifest in the same commit as the change.
  - The lagging client gets an `@parity` backlog line naming the action id.
  - Deviations are never silent.
- `.claude/scripts/check-parity.sh` is advisory only.
  - It warns when `keymap.rs`, `input.rs`, `keys.ts` or `ui/` files change and the manifest does not.
  - `.claude` is frozen, so this waits for the human decide line.
- ADR 0031 records the convention.

## Sources for seeding
- Current TUI: `crates/txtodo-tui/src/input.rs`, `ui/list.rs`, `ui/conflicts.rs`, `ui/offers.rs`.
- Current desktop:
  - `apps/desktop/src/lib/components/FileView.svelte` (Mod-Enter, Mod-s, Esc, and CM6 defaultKeymap Alt-Up/Down)
  - `DetailView`, `WorkspaceSwitcher`, `UniversalView`, `src/devices/*`
  - `src-tauri/src/quick_add.rs`, `tray.rs`
- Revamp: `tasks/desktop-ui-revamp/notes.md:235-240` and the c2 footer hints (`c2-prompt.html:335-339`).

## As built
- `specs/client-parity.toml`: 89 actions, 12 screens. ADR 0031 accepted 2026-09-25 (decide line signed).
- `crates/txtodo-tui/tests/parity.rs`: every bound command has a row with `tui.status` done or differs, same scope, same keys (`tui.keys` when the row has them, else the shared ones); every done row is bound; every differs/na row has a deviation.
  - It reads the manifest with a hand parser for the subset, like `keys.parity.test.ts`. So the `toml` dev-dep in the deps decide line is no longer needed; only `qrcode` is left in it.
  - Checked it fails: changing `G` to `H` for `list.last` fails on `list.last: keys`.
  - A planned row may carry a deviation ahead of time (`prompt.focus`), so the deviation rule is one way only.
- `apps/desktop/src/lib/keys.parity.test.ts`: the manifest rules run; the keys.ts half is skipped until keys.ts exists.
- Open: `check-parity.sh` waits on its decide line (`.claude` is frozen).
