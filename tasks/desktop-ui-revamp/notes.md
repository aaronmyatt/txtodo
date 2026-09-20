# Desktop UI/UX revamp: the c2-prompt design, end to end (plan M11)

Status: **deferred 2026-09-19 (human call).** The design is signed off; the build is deprioritised
until the daemon/desktop stabilisation work lands. Nothing here is started. The root line is `(D)`
and the last open line in file order. It carries no `@human` tag (same as the other deferred items),
so a loop that reaches it would pick it up: skip it until the human un-defers it. Below, six
`@human` lines are decisions (they block only their dependants) and the second-to-last is the
human pass on a real build.

## Goal

Rebuild `apps/desktop` around the new logo (`assets/brand/`) and the `c2-prompt.html` mockup,
**without changing what editing is**: the main view stays a raw, directly-editable `todo.txt`
buffer. New on top: a header search bar, a prompt-bar Quick Add, the logo as a live progress meter,
a rebuilt Universal page, a real Settings page, and an ink-on-paper theme.

## The mockups (the spec)

- Live in `apps/desktop/design-mockups/`, which is **gitignored** (`.gitignore:41`). They exist only
  on the machine that made them. First sub-line: commit them to `docs/design/desktop-ui-revamp/`.
- `c2-prompt.html` is the target. Its parts: `c2/universal.js`, `c2/settings.js`, and `shared/`
  (`txtodo.css` themes + editor, `txtodo-ui.js` engine, `todotxt.js`, `data.js`).
- `a-paper.html`, `b-tile.html`, `c-prompt.html` are history (the human liked C best; C2 is its
  second pass). Do not port A's/B's gutter done-toggle, B's rail or A's slide-over.
- View: `python3 -m http.server 8765` in that dir, open `/c2-prompt.html` (needs internet: CDN).
  `?selftest` runs its 48 in-page checks; `node shared/selftest.mjs` runs the pure logic.
- The mockup editor is a transparent `<textarea>` over a highlighted mirror. **The app keeps
  CodeMirror 6.** Take the look and behaviour from the mockup, never its editor code.
- Things the mockup fakes and the app must not copy: the token secret (server-side only), QR
  pattern (real `pair_offer` QR), "last used" on tokens (no such field), device list, sample data,
  `execCommand` edit tricks, JS date maths for Universal (use the daemon's DTO), the toast Undo
  (re-toggle; the app should use the daemon's real Undo).

## Design language (what "the new flavour" means)

- Pure ink and paper (light `#fff`/`#000`, dark `#000`/`#fff`); `primary` is the inverse tile.
  Signal colours only where meaning needs them: overdue red, due-soon amber, sync green, plus the
  existing `--tok-*` syntax palette (unchanged).
- Round caps mean round shapes: pill buttons/badges/selectors, `--radius-box` 1.1rem, 2px borders,
  flat (`--depth` 0), inverse selection. Icons: monoline, 24 grid, stroke 2.75, round caps/joins.
- JetBrains Mono 700 for the wordmark and headings, 400/500 for buffer and chrome, plain sans only
  for prose. The logo's letters are content: `t` = open, `x` = done (`t 9 · x 3`, lowercase).
- Search hit = the inverse tile in miniature (ink block, paper text); misses fade to 30%.

## Decisions needed (each is a `@human` line; my pick first)

1. **Tailwind 4 + DaisyUI 5 at build time**, or plain CSS. Pick build time (the mockups use them;
   the themes are a thin token layer), gated on the CSS/webview audit.
2. **Detail as bottom panel, or a page.** Pick the panel (buffer and scroll stay visible). Plan
   §3.3 says "a page, not a modal" and the detail e2e specs assume a page, so this amends the plan.
3. **Which "proposed" settings ship.** Pick text size, line numbers, long-line toggle first; editor
   font and line height second; skip save-on-blur (blur/Cmd+S commit is the editing contract).
4. **Fold `/devices` and the sidebar Activity tab into `/settings`**, redirect `/devices` one
   release. Pick yes.
5. **Fonts.** Self-host JetBrains Mono 400/500/700, system UI font for prose, no Inter. Today only
   weight 700 loads, from Google Fonts (`app.html`).
6. **Quick Add.** Keep the global-hotkey window for when the main window is hidden or unfocused
   (the app is menu-bar resident, `tray.rs`); the prompt bar takes the hotkey only when the main
   window is frontmost.

## Tooling and theme

- Today: SvelteKit 2 + Svelte 5 runes, Vite ^8.0.16, adapter-static, CM6, no Tailwind. Styling is
  scoped component CSS plus custom properties in `src/app.css` (`--color-*`, `--tok-*`,
  `--font-*`), switched by `<html data-theme="light|dark">` from `$lib/stores/theme.ts`.
- Spike first: does `@tailwindcss/vite` + daisyUI 5 build under Vite 8 and adapter-static, with
  Vitest and Playwright still green? Verify against the versions actually installed.
- Map, do not replace. Keep every existing `--color-*`/`--tok-*` name (CM6's theme reads
  `var(--tok-*)`, scoped styles read `var(--color-*)`), and add DaisyUI's names next to them:

  | mockup token (DaisyUI) | app token today | note |
  |---|---|---|
  | `--color-base-100` | `--color-bg` | true `#fff` / `#000` |
  | `--color-base-200` / `-300` | `--color-surface` / `--color-surface-muted` | greys go neutral, not blue-grey |
  | `--color-base-content` | `--color-text` | already `#000`/`#fff` (brand pass) |
  | `--color-primary` / `-content` | new | the inverse tile |
  | `--color-error/warning/success` | `--color-danger/-warning-text/-success` | keep AA on both themes |
  | `--tok-*` | `--tok-*` | unchanged values |

- DaisyUI ships built-in `light`/`dark` themes: set `themes: false`, define ours, keep theme.ts's
  `data-theme="light|dark"` (or rename both sides together).
- CDN lessons: unlayered CSS beats Tailwind utilities (overridable defaults go in `@layer base`);
  the CDN build's DaisyUI colours only had `hover:` and `/10../90` steps (npm plugin: confirm).
- **CSS floor.** The mockups use `color-mix()`, `:has()`, cascade layers, `oklch()`, `isolation`.
  Tauri 2 uses WKWebView (macOS), WebKitGTK (Linux), WebView2 (Windows); `tauri.conf.json` sets no
  `bundle.macOS.minimumSystemVersion`. Tailwind 4 itself needs a recent Safari (16.4+ at the time of writing). Find the floor, set
  it, set Vite's `cssTarget`, write it in the README. Windows is out of scope (README).
- Fonts: `@fontsource/jetbrains-mono` 400/500/700; remove the Google Fonts `<link>` in `app.html`
  (`csp` is null, so nothing stops it; offline first paint is the reason). Icons: one `Icon.svelte`
  with a name union; size in CSS, not `width`/`height` attributes.
- `Mark.svelte`: 4 paths (`M20 6 L20 42 C 20 47, 23 49, 28 49`, `M8 20 L28 20 L40 34`,
  `M58 22 L80 44`, `M80 22 L58 44`, viewBox `0 0 100 64`, stroke 7, round). With `progress` 0..1
  draw the strokes in order (`pathLength=1`, `stroke-dasharray: 1 2`, hidden offset `1.05`, not
  `1`, or round caps paint a dot). First paint must not animate. Reduced motion: no transition.

## Screen map: mockup piece to real code

| mockup piece | real code today | change |
|---|---|---|
| header (mark, workspace menu, search, nav, pin/theme/help) | `MainView.svelte` `.top-nav`, copies in `routes/universal`, `devices`, `help` | one `AppShell.svelte` in `+layout.svelte` |
| workspace dropdown | `WorkspaceSwitcher.svelte` (drawer, Workspaces + Activity tabs) | header dropdown, counts, current marker; Activity tab moves to Settings |
| nav join Tasks / Universal / Settings | text links "Universal view", "Devices & agents" | three buttons, icon-only under ~1280px |
| buffer | `FileView.svelte` (CM6) | gutter, active line, block caret, search decorations (below) |
| sub-toolbar (file, counts) | none | new strip above the buffer |
| prompt bar (Quick Add inline) | `QuickAdd.svelte` + `EditPopover.svelte` in a separate window | new `PromptBar.svelte`; shared single-line editor |
| chips row | `EditPopover.svelte` `CHIPS` | same nine chips; float above the bar while focused |
| live mark | none | `Mark.svelte`, progress from `listFiles()` `done/total` |
| status footer | scattered banners | one footer: synced / unsaved / saving / saved, Ln/Col, key hints |
| detail bottom panel | `DetailView.svelte` page | per decision 2 |
| Universal | `UniversalView.svelte` (priority groups, context chips) | rebuilt; needs a richer DTO |
| Settings | `routes/devices` + `Devices.svelte`, `Tokens.svelte`, `ActivityTab.svelte`, theme/pin toggles | one page, seven cards |
| banners (daemon, conflict, skill hint) | `MainView` banner, `ConflictBanner`, `SkillHintBanner` | inverse strips; `ConflictReviewSheet` becomes a modal |
| toast with Undo | none | small store + `undo` Tauri command |

## Editor port (CodeMirror 6)

Keep the whole editing contract (see Invariants). Additions only:

| mockup behaviour | CM6 implementation |
|---|---|
| line numbers + hairline | `lineNumbers()`; border on `.cm-gutters` |
| active-line background | `highlightActiveLine()` or a line decoration; keep the existing hover line |
| block caret | CM6 draws its own cursor (`drawSelection`), so style `.cm-cursor` as a block in CSS. `caret-shape` is Chromium-only and WKWebView ignores it, so do not use it |
| hit marks | `Decoration.mark({class:"cm-hit"})` on hit ranges; viewport-bounded like `decorations.ts` |
| dim non-matching lines | `Decoration.line({class:"cm-dim"})` on non-blank non-matching lines |
| current hit | line class + `EditorView.scrollIntoView(pos, {y:"center"})` without moving focus |
| text size / line height | CSS vars on the editor host; use px line height (row maths in decorations assume it) |
| 100-char hint on/off | class toggling the existing `cm-todotxt-long-line` style |

- Search state is a `StateField` holding the parsed query, set by an effect from the header input.
  The hit list (for `n/m` and next/prev) is computed lazily (idle or chunked over `doc.iterLines`)
  so 10k lines never block typing (budget: `e2e/perf.spec.ts`). Every `FileView` (main, detail
  sub-list, parent line) reads the same `editorPrefs` store.

## Search

- One field in the header, press `/` to focus, `Esc` clears then blurs. Placeholder and scope
  follow the page: Tasks = current buffer, Universal = all workspaces, Settings = its cards.
  Tasks and Universal share one query (search follows you); Settings keeps its own.
- Semantics: whitespace-separated terms, all must match, case-insensitive substring; `is:open` /
  `is:done` are the only operators. **Add `-term` exclusion** to match `txtodo list`
  (`crates/txtodo-cli/src/commands/list.rs`) and align with root item `mcp-query-language-real`.
  One documented `matchesQuery(line, query)` in `src/lib/todotxt/query.ts`, with table tests.
- Display filtering, not parsing (like `lineInfo.ts`, "never a task data model"); facts that need
  real parsing (due, projects, done, ref progress) come from the daemon DTOs, not JS.
- Tasks: hits invert, other lines fade, count pill `1/3`, toolbar says "3 of 13 lines match",
  Enter / Shift+Enter step (focus stays in the input), nothing is ever hidden or reordered.
- Empty query: a panel of filter chips (top contexts, projects, `(A) (B) (C)`, `is:open`,
  `is:done`, `due:`) plus recent searches; once you type the panel steps aside so it never covers
  the matches. Chips use `mousedown.preventDefault` to keep input focus.

## Universal

- Data today: `universalTasks()` returns open tasks only (`workspace_id`, `workspace_root`,
  `line_number`, `priority`, `contexts`, `description`), root `todo.txt` only; nested `ref:`
  sub-lists are a deliberate cut (`tasks/desktop-universal-view/notes.md`), keep it.
- Needs: `done`, completion date, `due`, `projects`, `task_id`, ref progress, and an
  `include_done` argument (for "show done" and the `x done` stat). Change `dto_universal.rs`,
  `commands_universal.rs`, `daemon.ts`, and the real-daemon test `src-tauri/tests/universal_view.rs`.
- Page: stat strip (overdue, due within 7 days, `t open`, `x done`); group by priority (default),
  due, project, context, workspace; workspace chips (at least one stays on) and context chips;
  show-done toggle; rows = round mark, description with token colours, due badge (overdue red,
  today amber, soon amber-soft, later ghost), ref `n/m` pill, workspace tile.
- Sort in a group: not done first, then due date, priority, line. Due label: `overdue 2d` /
  `today` / `tomorrow` / `in 4d` (up to 7) / `Oct 15`, local date via `localToday()` (ADR 0011), in
  `src/lib/todotxt/due.ts` with tests.
- Complete/reopen from the row: `applyMutations(path, [{kind:"complete", task, today}])` against
  *that row's* workspace (so the DTO must carry `task_id`). Reopen has no `Mutation` variant; use
  `edit` with `toggleComplete` text (`editPopoverLogic.ts`) or add a variant. **Undo** via the
  daemon's Undo (CLI has `txtodo undo`); the desktop bridge has no `undo` command yet, add one.
- Keyboard: `j`/`k`/arrows/Home/End move, Enter opens (existing `pendingUniversalNav` +
  `switchWorkspace`), `x` completes; roving tabindex; focus survives a re-render. Refetch on window
  focus and after own mutations: one bounded fetch, not a live tail (as `opLogAll`).

## Settings (seven cards, sticky nav with scroll-spy)

| card | rows | real or proposed |
|---|---|---|
| General | pin on top | real (`pin.ts`) |
|  | keep running in the menu bar | real behaviour today (hide-on-close, tray Quit); a toggle needs config in `tray.rs`/`lib.rs`, otherwise show it as an explanation |
|  | save when the buffer loses focus | proposed (skip, decision 3) |
|  | daemon status + Restart | real (`daemonStatus`, `retryConnect`) |
| Appearance | theme System/Light/Dark | real (`themePreference`) |
|  | text size, line height, line numbers, 100-char hint, editor font | proposed (`editorPrefs`) + live preview buffer |
| Workspaces | list, open, add, remove | real (`listWorkspaces/addWorkspace/removeWorkspace/switchWorkspace`) |
| Shortcuts | table | generated from `keys.ts` + the Rust hotkey; global hotkey is Cmd/Ctrl+Shift+Space (`quick_add.rs`), "configurable later" |
| Devices | pairing (QR, countdown, scan, SAS both sides) | real (`pairOffer`, `pairAccept`, `pairConfirmSas`) |
|  | paired-devices list + revoke | **no RPC yet** (CLI has `txtodo device list/remove`); revoke = group-key rotation, needs a confirm |
| Tokens | list, create, revoke | real (`tokenList/tokenCreate/tokenRevoke`) |
| Activity | newest 200 ops across workspaces | real (`opLogAll`) |

- Token dialog gaps vs the mockup: the real `tokenCreate(name, scopes, expires)` takes an
  **expiry** (RFC 3339, empty = never), so add an expiry control. Scopes are the closed set from
  design §6.2 (`read`, `write:add|complete|edit|delete`, `raw`); `project:`/`context:`/`file:`
  restrictors ride in the same list. The secret comes only from the create response and is never
  logged or re-shown. No "last used" column unless the daemon grows the field.
- Editor prefs persist in `localStorage` like the theme (the quick-add webview shares the origin).
  Deep links `/settings#tokens`; header search hides cards missing a term (`data-kw`); pin the nav
  highlight to the last card when scrolled to the end.

## Prompt bar and Quick Add

- `PromptBar.svelte` at the foot of the main window: single-line editor (Enter adds, Esc back to
  the buffer), nine chips, the strict-check hint (never blocks, design §2.3), live mark on the
  left. Enter = `applyMutations(root, [{kind:"add", line}])` on the *current* workspace; the
  daemon stamps date and identity, never the UI.
- Chips float above the bar while focused (no layout shift); editors need `isolation: isolate` or
  their z-indexes cover the chip row (found in the mockup).
- Extract the single-line editor + chip logic from `EditPopover.svelte` so PromptBar, the Quick
  Add window and the popover share it; `editPopoverLogic.ts` stays the pure core.
- Global hotkey (`quick_add.rs`, `commands_window.rs`): main window frontmost = focus the prompt
  bar; otherwise the separate always-on-top window as today. Keep the dirty-popover guard
  (`setMainPopoverDirty`). The window is hidden, not destroyed, and remounts on `quick-add-shown`.
- Live mark: progress = `FileInfo.done / total` for the current root `todo.txt` from `listFiles()`
  (plan §3.2.5: blanks excluded), refreshed on `Watch` changes. Zero tasks draws nothing.
  Tooltip `n of m done`.

## Detail view

- Keep the pinned parent, notes, recursive sub-list, breadcrumb, lazy `ref:` creation and "mark
  parent done" offer. If decision 2 picks the panel: a bottom split of `MainView` (~55% height,
  buffer stays visible, close button + Esc) and update the detail/breadcrumb e2e specs; if the page
  stays, only restyle.

## Data and bridge changes (Rust, `apps/desktop/src-tauri`, unfenced)

- `universal_tasks`: richer DTO + `include_done` (above). `undo` over the daemon's Undo op.
- `list_devices` (+ revoke) for the paired-devices list. Removal rotates the group key: a
  trust-boundary operation, so a confirm dialog and a security review line.
- `quick_add_shortcut` (read-only string) for Shortcuts; optional `keep_running` config if the
  tray toggle is real.
- Anything touching a `crates/` crate needs that crate's lease (slice fence); `apps/desktop` is
  unfenced.

## Keyboard map (single source: `src/lib/keys.ts`)

`/` search · `g t` / `g u` / `g s` go to Tasks / Universal / Settings (900 ms chord) · `Cmd+Shift+Space`
Quick Add (global) · buffer: `Cmd+Enter` detail, `Cmd+S` save, `Esc` discard · search: `Enter` /
`Shift+Enter` next/prev, `Esc` clear · Universal: `j k Home End`, `Enter`, `x`. Bare-letter keys
are ignored inside inputs, CM6 and dialogs. The help page and the Shortcuts card render this table.

## Invariants to preserve (one regression test each)

- Click a line, type; the whole document is the live buffer. Blur or `Cmd/Ctrl+S` commits through
  `computeDelta` as intent-level `Mutation`s, never a whole-string write; `Esc` discards to the
  baseline. A no-op save does nothing.
- `Cmd/Ctrl+Enter` and double-click open the detail view for the line.
- `id:` tags hidden; done lines dimmed + struck (not the marker/dates); `ref:` shows `n/m` or a
  notes glyph; trailing blank line shows "Add a line…"; lines past 100 chars get the wavy hint.
- Buffer is read-only while a `needs_review` flag is pending, unless already dirty.
- A concurrent `Watch` change never overwrites a dirty buffer (`desktop-concurrent-edit-loss`).
- Quick-add hotkey guard: a dirty main popover focuses the main window instead.
- Closing the window hides it; only the tray's Quit exits.

## Testing plan

- Vitest for each new pure helper, in the same commit: query matcher, due label, grouping/sort,
  `editorPrefs`, keymap chords, Mark progress, Universal stats.
- Tauri shim (`src/lib/mock/*`) grows the new commands so `npm run dev` and Playwright run without a
  daemon. Playwright: search, Universal, Settings, prompt bar + chips, Quick Add window, detail,
  conflict read-only; fix the specs the shell breaks (`workspace-switcher`, `detail`, `breadcrumb`,
  `activity-tab`, `popover`).
- Visual: re-baseline `e2e/visual` and add Universal, each Settings card, search-active, Quick Add,
  chips, light + dark (snapshots are `-darwin` only today; see `desktop-visual-regression`). Perf:
  `e2e/perf.spec.ts` 10k-line fixture with gutter, search and block caret.
- Accessibility: focus order and 2px ring, `aria-current`, dialog focus traps, AA contrast for the
  60%-opacity tones in both themes, `prefers-reduced-motion`, live regions for hits and Undo.
- Human pass on a real build (macOS + Linux), including pairing with a second device.

## Order of work

The sub-backlog is in dependency order: decisions and mockup record, foundation (CSS audit,
Tailwind spike, themes, fonts, icons, Mark), shell, editor, prompt bar and Quick Add, Universal
(DTO first), Settings (Devices and Tokens last: security review), docs, tests/perf/a11y, cleanup.
Editor and prompt bar can run in parallel after the shell; Settings needs `editorPrefs`.

## Risks

- **Webview drift:** Tailwind 4/DaisyUI 5 and the CSS above assume a modern engine; the CSS-floor
  line settles it before any code lands. Vite 8 + the Tailwind plugin is unverified (spike line).
- **Snapshot churn:** every visual baseline changes at once; re-baseline in one commit.
- **10k-line perf** with search decorations and a gutter: stay viewport-bounded, compute hits lazily.
- **Scope creep:** each "proposed" setting is a new persisted preference to test and document.
- **Two Quick Adds** (window + prompt bar) must not diverge: one shared component.
- **Security surface:** device revoke and token creation are trust-boundary UI; secrets are never
  logged or generated client-side, and revoke asks first.

## Not in scope, and open questions

- Out: A/B mockup features (gutter done-toggle, icon rail, slide-over); Windows (README); nested
  `ref:` sub-lists in Universal (existing cut); sync/daemon protocol, grammar or Lezer token names;
  a configurable global hotkey.
- Open: does "keep running in the menu bar" become a real setting? A `ws:` search operator? Reopen
  as a new `Mutation` variant (one op in the log, preferred) or `edit`? Mockups live in
  `docs/design/desktop-ui-revamp/` or un-ignored in place (either way, note the CDN dependency).

## As built

_Empty until work starts._
