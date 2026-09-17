# Dismissable left-nav workspace switcher (storyboard screens 07/08)

## Goal

Replace `WorkspaceSwitcher.svelte`'s anchored dropdown with a dismissable left-nav sidebar, and
scaffold the tab head (`Workspaces` / `Activity`) it will share with `desktop-activity-cross-workspace`.
This came out of a desktop-experience storyboard (Artifact, not checked in) that walked the app's
interactions end to end; the sidebar and its Activity tab were the two screens with no code behind
them yet.

## Current state (read before writing anything)

- `apps/desktop/src/lib/components/WorkspaceSwitcher.svelte` — a `<button class="trigger">` that
  toggles a `position: absolute` `.panel` anchored `top`/`right` of itself (lines 89–124). No Esc
  handler, no outside-click handler: today, clicking anywhere outside just leaves it open.
- `apps/desktop/src/lib/stores/workspaces.ts:9` — `currentWorkspaceRoot` writable, read/written
  around `switchWorkspace`. Untouched by this task.
- `switchWorkspace(root)` only changes the selector attached to calls made *after* it resolves
  (`WorkspaceSwitcher.svelte:5-7`); `MainView` remounts `FileView`/`ConflictBanner` via
  `{#key $currentWorkspaceRoot}`. Untouched by this task — this is a shell/positioning change, not
  a behavior change.
- `listWorkspaces()` / `addWorkspace()` / `removeWorkspace()` (`$lib/daemon`) are the same calls the
  new sidebar's list and add-form use. No new daemon commands.

## Design

### Shell, not logic

Keep `pick`/`add`/`remove`/`refresh` exactly as they are — only where the markup lives and how it
opens/closes changes. Concretely:

- The trigger becomes a small icon-only toggle button (▤) in `MainView`'s titlebar/top-nav, not a
  text button showing the current root — the current root is already visible in the window's own
  path/breadcrumb, so repeating it on the trigger was redundant once the panel becomes a persistent
  rail rather than a transient menu.
- The `.panel` becomes a full-height sidebar (`position: relative` flex child, or `position: fixed`
  inset to the window's content area — pick whichever `MainView`'s existing layout makes simpler,
  there's no daemon-facing reason to prefer one) that pushes or overlays the file view, sliding in
  from the left. It is no longer anchored to the trigger's coordinates.

### Dismissal

Three ways in, matching the storyboard caption:

1. The toggle button again (existing `open` boolean, just flip the affordance).
2. `Esc` while the sidebar has focus.
3. A click outside the sidebar's bounds — needs a `window`-level `click` (or `pointerdown`) listener
   registered only while `open`, removed on close, checking the event target against the sidebar's
   own DOM node (a `bind:this` ref), the same pattern the edit popover will need for its own
   focus-trap work (plan §3.3) if that hasn't landed yet — don't invent a second idiom if one
   already exists in this codebase for "click outside a floating panel."

### Tab head

Add the two-tab bar (`Workspaces`, `Activity`) at the top of the sidebar. `Workspaces` renders
today's list + add-form (moved, not rewritten). `Activity` renders an empty/"coming soon" state in
*this* task — its real content (the cross-workspace, agent-aware feed) is
`desktop-activity-cross-workspace`'s job, deliberately split off because it needs its own daemon
fan-out design, not just a UI shell.

### Accessibility (plan §3.3 floor)

- Every action reachable from the keyboard: tab bar is a `role="tablist"`/`role="tab"` pair (or the
  Svelte equivalent already used elsewhere in this codebase, if one exists — check before adding a
  second tab-list pattern), workspace list stays a real list of buttons.
- Focus moves into the sidebar (first focusable element) on open, and returns to the toggle button
  on dismiss — the same "never leave focus stranded" rule the edit popover's focus trap follows.

## Explicitly out of scope

- The Activity tab's actual content, its data source, and the agent/human principal distinction —
  all of `desktop-activity-cross-workspace`.
- Any change to `switchWorkspace`, `addWorkspace`, `removeWorkspace`, or the daemon-side
  `WorkspaceCatalog`/`WorkspaceRegistry` — this task is desktop-frontend-only.
- A folder-picker dialog for "add" — still out of scope repo-wide (`@tauri-apps/plugin-dialog` isn't
  a dependency), unchanged from today.

## Acceptance

- Toggling the titlebar icon opens/closes the sidebar; `Esc` and an outside click both dismiss it.
- Workspace switch/add/remove behave identically to today (same daemon calls, same remount-on-switch
  contract) — only the container moved.
- Tab bar renders both tabs; `Activity` shows a placeholder, not an error, until the other task lands.
- Playwright coverage (if `WorkspaceSwitcher` has any today — check `apps/desktop/tests/` before
  assuming there is none) re-pointed at the new markup, plus new cases for Esc/outside-click dismiss
  and tab switching.

## As built (2026-09-17)

- `WorkspaceSwitcher.svelte` rebuilt in place (same component/import site, `MainView`'s
  `.top-nav-actions` unchanged): the trigger is now an icon-only `▤` button; the panel is a fixed,
  full-height `div role="menu"` (not `<aside>` — svelte-check's a11y rule flags a landmark element
  given an interactive role, same fix `ConflictReviewSheet.svelte` already uses for its own
  `role="dialog"`) sliding in from the left via `svelte/transition`'s `fly`. Esc and outside-click
  (`window` listeners registered only while open) both dismiss; focus moves to the first tab on
  open and returns to the toggle on close — same pattern `ConflictReviewSheet.svelte` uses.
- Tab bar (`role="tablist"`/`role="tab"`/`role="tabpanel"`): `Workspaces` renders the moved
  list+add-form unchanged; `Activity` is a placeholder pending `desktop-activity-cross-workspace`.
- New `apps/desktop/e2e/workspace-switcher.spec.ts`: toggle+Esc+focus-restore, outside-click
  dismiss, tab switching — no prior Playwright coverage existed (confirmed by checking `e2e/`
  first, per this task's own notes).
- **Found and fixed a real, pre-existing break in the whole Playwright suite** while writing that
  spec: `e2e_bridge.rs`'s `DaemonClient::connect(&sock)` call was a straight compile error
  (`connect` grew a required `selector` param under `mcp-multi-workspace-gateway`) — every
  Playwright test in this repo was silently broken (never caught since `apps/desktop` has no CI
  coverage, `desktop-stack-gaps`). Fixed with a `Path`-selector `connect` call mirroring
  `commands.rs::connect_and_store`'s own pattern (an initial attempt using `None` also compiled but
  failed at runtime — "no workspace is open" — since `ensure_daemon` spawns true global mode with
  zero workspaces open; only a `Path` selector actually gets one auto-registered/opened). Also
  wired `list_workspaces`/`add_workspace`/`remove_workspace`/`switch_workspace` into the bridge
  (`e2e_bridge/workspace.rs`, split out for the file-length budget) and into
  `apps/desktop/e2e/fixtures.ts`'s implicit pass-through (`e2e/shim/core.ts`'s `default` case
  already forwards unknown commands, no change needed there) — none of the four existed on the
  bridge before, since nothing had exercised `WorkspaceSwitcher` over Playwright until now.
- **Known gap surfaced, not fixed here (flagged to a human as a follow-up task):** `ensure_daemon`
  spawns one real, machine-global `txtodod` (true global mode) shared across every Playwright run,
  and `fixtures.ts::dispose()` still calls `killDaemon(dir)` — a leftover per-directory-daemon
  assumption from before the global-daemon migration — and never un-registers the workspace it
  registered. Every test run leaks a registry.db entry and the daemon process itself outlives the
  run. Found ~52 leaked entries (my own session's repeated test runs) in the real, persistent
  registry and cleaned them up by hand (`txtodo workspace remove` in a loop) plus killed the stray
  daemon process — a one-off manual fix, not a structural one. Out of this task's scope (a Svelte
  shell change), so flagged as its own follow-up rather than fixed inline.
- Visual regression goldens (`e2e/visual/main-view.spec.ts` and 2 others) now fail — expected: the
  trigger changed from a wide text button to a small icon, shifting the top-nav's other links left
  by a few px, which cascades into the "95% pixels differ" reading (confirmed by eye: the actual
  screenshot renders correctly, just shifted). Deliberately **not** regenerated here —
  `playwright.config.ts`'s own comment: golden updates are human-reviewed only
  (`test:visual:update`), never automatic. A human should run that and review the diff.
