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
