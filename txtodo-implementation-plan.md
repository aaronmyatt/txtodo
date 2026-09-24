# txtodo — implementation plan

This document is written for a coding agent. It is deliberately explicit. Read `txtodo-design.md` first; where the two disagree, this plan wins because it is newer and incorporates UI decisions made after the design doc was written (§3).

---

## 0. How to work from this plan

**Order.** Milestones M0–M10 are sequential. Do not start a milestone until the previous milestone's acceptance criteria pass in CI. Inside a milestone, tasks are listed in a sensible order but may be reordered.

**Definition of done for every PR.**

- Code compiles with `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check`.
- New behaviour has tests. Bug fixes add a regression test first.
- Public items have doc comments. No `TODO` without an issue link.
- `just check` passes (see M0).
- The PR description names the milestone task it closes.

**Stop and ask before** doing any of the following. Open a question in `docs/questions.md` and continue on other work:

- Changing anything that is written to `todo.txt` or `notes.md`.
- Adding a new `key:value` tag or changing the meaning of an existing one.
- Adding a dependency with a native/C build step, or any dependency over 1 MB compiled into `txtodo-core`.
- Moving code across crate boundaries defined in §2.
- Anything that would violate a rule in §1.

**Non-negotiables (from the design doc, restated).**

1. The files are the truth. Everything under `.txtodo/` is rebuildable from them.
2. Byte-preserving round trip for untouched lines.
3. Never write a construct the todo.txt spec doesn't define.
4. App metadata lives only in the documented tags: `id`, `pri`, `due`, `t`, `rec`, `h`, `ref`.
5. Line order is data. Blank lines are entries.
6. Preserve file hygiene (line endings, BOM, trailing newline); never canonicalise as a side effect.
7. Never silently lose text the user typed.

**Conventions.**

- Rust, edition 2024, stable toolchain pinned in `rust-toolchain.toml`. MSRV = the pinned version.
- Cargo workspace; one crate per §2 entry; `#![forbid(unsafe_code)]` everywhere except `txtodo-ffi`.
- Errors: `thiserror` in libraries, `anyhow` in binaries. Never `unwrap()` outside tests.
- Logging/tracing: `tracing` with structured fields; no `println!` outside the CLI's output path.
- Async: `tokio`. The daemon is async; `txtodo-core` is sync and has no async dependency.
- Tests: unit tests beside code; integration tests in `tests/`; property tests with `proptest`; fuzz targets under `fuzz/`.
- Commits: conventional commits (`feat(core): …`, `fix(sync): …`, `test(cli): …`).
- Architecture decisions go in `docs/adr/NNNN-title.md` (template in M0). Decisions in §1 of this plan are ADR 0001–0012; write them up as the first task of M0.

---

## 1. Decisions already made — do not relitigate

| # | Decision | Rationale |
|---|---|---|
| 1 | Language: Rust for everything below the UI. | One core, many bindings. |
| 2 | CRDT library: **Loro**. Wrap it behind a `Doc` trait so it could be swapped, but do not build the abstraction until a second implementation exists. | Native movable list (line order), text (description, notes), map. |
| 3 | Networking: **iroh** for QUIC, hole-punching and relay; **mDNS** for LAN discovery (`mdns-sd` crate). | Covers three transports with one dependency. |
| 4 | Storage: **SQLite** via `rusqlite` (bundled), WAL mode. | Ubiquitous, single file, works on mobile. |
| 5 | MCP: the official Rust MCP SDK (`rmcp`; verify current crate name/version before adding). Transports: stdio and Streamable HTTP. | Don't hand-roll a protocol. |
| 6 | Local IPC: unix domain socket / Windows named pipe carrying **gRPC** (`tonic`). The REST/JSON mirror is generated from the same protobuf via `tonic-web` or a thin axum shim. | One schema, two surfaces. |
| 7 | Desktop UI: **Tauri 2** + **Svelte 5** + **CodeMirror 6**. The main view is a read-only CodeMirror document with a custom todo.txt language (Lezer grammar generated from `specs/todotxt.abnf`). | CM6 gives line numbers, wrapping, virtualisation, search, and decorations for free; the "interactive text file" feel is native to it. |
| 8 | Mobile UI: iOS SwiftUI over a `UITextView` with `NSTextStorage` highlighting; Android Jetpack Compose `BasicTextField` with `AnnotatedString`. Both highlight using `txtodo_core::tokenize` via uniffi so token boundaries are identical everywhere. | The core owns the grammar; UIs only paint. |
| 9 | ~~Task identity: `id:<ULID>` tag, tagged mode only for M1–M7. Sidecar (purist) mode is M10.~~ **Reversed 2026-09-13** (`docs/questions.md` Q2): sidecar mode (no `id:` tag; identity by fingerprint re-matching, `crates/txtodo-model/src/identity.rs` + `crates/txtodo-daemon/src/identity_*.rs`/`reconcile_sidecar.rs`) is the default for every new workspace; tagged mode is now the opt-in (`--identity-mode tagged`, or auto-detected when a workspace already carries `id:` tags). Both are fully built and tested, not just tagged. | A plain, unmanaged todo.txt should work with `txtodo` with zero `id:` metadata written into it (user request, superseding "ship the robust path first"). |
| 10 | Ports and names: MCP HTTP on `127.0.0.1:8636`; gRPC on the socket only; metrics on `127.0.0.1:8637`. mDNS service `_txtodo._udp` (sync); MCP is never advertised (it is loopback only since 2026-09-20, `tasks/mcp-local-only`). Daemon binary `txtodod`, CLI `txtodo`, config at `$XDG_CONFIG_HOME/txtodo/config.toml`, state at `<workspace>/.txtodo/`. | Fixed so docs and tests can rely on them. |
| 11 | Dates: the daemon's local date at write time for `creation_date` and `completion_date`, formatted `YYYY-MM-DD`. No time zones in the file, ever. | Spec. |
| 12 | Detail files: the `ref:` directory convention in §3.2. | Agreed in design review. |

---

## 2. Repository layout and crate boundaries

```
txtodo/
├── Cargo.toml                 workspace
├── rust-toolchain.toml
├── justfile                   check · test · fuzz · bench · corpus · release
├── crates/
│   ├── txtodo-core/          parse · tokenize · model · format · diff        (sync, no_std-friendly, no I/O)
│   ├── txtodo-query/         query language: parse → plan → evaluate          (depends: core)
│   ├── txtodo-model/        workspace tree, Task ids, HLC, op types          (depends: core)
│   ├── txtodo-store/        SQLite op log, snapshots, projection cache       (depends: model)
│   ├── txtodo-crdt/         Loro document, ops ⇄ Loro, reconciler            (depends: model, store, core)
│   ├── txtodo-sync/         protocol, transports, pairing, crypto            (depends: model, store)
│   ├── txtodo-daemon/       txtodod: actors, watcher, IPC server, MCP host       (depends: all above)
│   ├── txtodo-proto/        .proto files + generated gRPC types              (no deps)
│   ├── txtodo-mcp/          MCP tools/resources/prompts, tokens              (depends: proto, query)
│   ├── txtodo-cli/          txtodo                                              (depends: core, proto; talks to daemon, falls back to direct file mode)
│   ├── txtodo-tui/          ratatui client                                   (M10)
│   └── txtodo-ffi/          uniffi + wasm-bindgen + cbindgen                 (depends: core, query)
├── apps/
│   ├── desktop/               Tauri 2 + Svelte 5 + CodeMirror 6
│   ├── android/               Kotlin, Compose
│   ├── ios/                   Swift, SwiftUI
│   └── web/                   PWA (M10)
├── relay/                     reference relay binary (M8)
├── specs/
│   ├── todotxt.abnf           the grammar; source of truth for parser tests and the Lezer grammar
│   ├── ref-directories.md     §3.2 of this plan, kept in sync
│   ├── conflicts.md           the conflict table as executable scenarios (see M4)
│   └── sync.tla               (M10)
├── corpus/                    real-world todo.txt files + expected token streams
├── docs/adr/                  architecture decision records
└── docs/questions.md          open questions for the human
```

**Dependency direction is strictly downward in the list above.** `txtodo-core` depends on nothing in the workspace and on no async runtime. If you find yourself importing `tokio` into `core`, stop.

---

## 3. UI and file-layout decisions from design review

### 3.1 The main view is the file

- Render `todo.txt` as a syntax-highlighted document with real line numbers. Blank lines are shown and numbered.
- Token colours (semantic names; each platform maps to its theme): `priority`, `date`, `completion-marker`, `project`, `context`, `tag-key`, `tag-value`, `id-tag`, `text`. Completed lines: whole line muted, description struck through, `x` and dates not struck.
- `id:` tags are hidden by default (rendered as a zero-width decoration on desktop; filtered from the attributed string on mobile). A header toggle shows them. They are always present in the edit field.
- Lines whose `ref:` directory has a `todo.txt` show a trailing indicator `n/m` (open/total, §3.2.5). Lines whose directory has only `notes.md` show a notes icon. Indicators are decorations, not text.
- Hover (desktop) or long-press (mobile) reveals a pencil affordance on the line.
- The last line of the document is always an empty "Add a line" row. Typing into it appends a line; the daemon stamps the creation date and `id:`.

### 3.2 Interactions

**Single click / tap on a line → edit popover.**

- Anchored below the line. Contains one single-line editor with the same highlighting, pre-filled with the *raw* line including `id:`.
- Token chips under the field: `(A)` `(B)` `(C)` `+` `@` `due:` `t:` `rec:` `x`. Priority chips *replace* the current priority (or remove it if the same chip is tapped). `x` toggles completion: on, it prepends `x <today> ` and, if a priority exists, removes it and appends `pri:<P>` (spec-conformant); off, it reverses that. Other chips insert the token at the caret with a leading space.
- Footer: `Line N · <device>, <relative time>` from the op log; Cancel and Save. Enter saves, Esc cancels. Save that produces a line identical to the original is a no-op (no op log entry).
- The popover validates in strict grammar mode and shows the parser's error inline (13 px, danger colour) but still allows saving in lenient mode with a quirk. Never block the user from saving text.

**Double click / double tap on a line → detail view** (§3.2 below). Desktop keyboard: `Enter` on a focused line = edit; `Cmd/Ctrl+Enter` = detail; `Cmd/Ctrl+E` = toggle *raw mode* (the whole document becomes editable inline; on blur or `Cmd/Ctrl+S` the buffer goes through the reconciler exactly like an external edit). Raw mode is a stretch goal in M7.

**Detail view.**

- Header: back, breadcrumb `todo.txt › line N` (nested: `todo.txt › 2 › tasks/q4-roadmap/todo.txt › 3`).
- The parent line pinned at the top, highlighted, with the same click-to-edit popover.
- Section `<ref>/notes.md`: a plain-text/markdown editor. No WYSIWYG. Light markdown highlighting (headings, list markers, code fences) is fine.
- Section `<ref>/todo.txt`: the same file-view component as the top level, fully recursive (its lines can be clicked, double-clicked, and have their own `ref:`). Header shows `n of m done`.
- Footer: absolute directory path and sync status.
- Both sections render even when their files don't exist yet. The first keystroke into either creates the directory and file (§3.2.3).

### 3.2 The `ref:` directory convention (normative; mirror to `specs/ref-directories.md`)

1. **Tag.** Key `ref`, value = a *slug*: `[a-z0-9][a-z0-9._-]*`, max 64 chars, no `/`, not `.` or `..`. Absolute paths and parent traversal are rejected by the parser as a quirk `invalid_ref` and treated as no ref.
2. **Resolution.** For a line in the workspace's root list, the slug names a directory in the workspace's refs folder: `refs_dir` in `<root>/txtodo.toml`, default `tasks`. `~/todo/todo.txt` line with `ref:q4-roadmap` → `~/todo/tasks/q4-roadmap/`. For a line in any other list, the slug names a directory in the same directory as that file, so nesting follows naturally: a line in `~/todo/tasks/q4-roadmap/todo.txt` with `ref:sync-section` → `~/todo/tasks/q4-roadmap/sync-section/`. `refs_dir = "."` puts the root list's ref directories beside it, the layout ADR 0012 first described. The two names are validated alike when the file is read: relative, `/` separators, no empty, `.` or `..` component, no `:`, not under `.txtodo`; `todo_file` names a file and is never `notes.md`; an empty value means the default. A value that fails is ignored and the default applies, and every client reads the daemon's layout (`WorkspaceLayout`) rather than its own copy of the file wherever a daemon runs.
3. **Contents.** Any of `todo.txt`, `notes.md`. All optional. Nothing else is managed or synced by txtodo (other files are left alone).
4. **Creation is lazy.** The tag is added and the directory created on the first write into the detail view's notes or sub-list. Slug = kebab-case of the description's plain words, truncated to 40 chars; on collision append `-2`, `-3`. The user can rename the slug in the edit popover; the daemon renames the directory atomically and rewrites the tag in the same op.
5. **Progress.** `done = completed lines in <ref>/todo.txt`; `total = task lines in <ref>/todo.txt`; blank lines excluded. Displayed as `open/total` on the parent line's indicator and `done of total` in the detail header.
6. **Parent completion is never automatic.** When `open == 0 && total > 0`, the UI offers "Mark parent done"; the daemon does nothing on its own. Completing the parent does not touch the sub-list.
7. **Archiving the parent** keeps the `ref:` tag on the archived line — it moves to the bottom of the same `todo.txt`, never to a second file — and leaves the directory in place.
8. **Moving a line between files** (e.g. `txtodo mv`, or drag on desktop) moves its directory to sit beside the destination file, applying the collision rule. If the move fails mid-way, the op is rolled back and the user is told.
9. **Dangling refs** (tag present, directory missing) are not errors: the detail view opens empty and lazy creation applies.
10. **Deleting a line** with a `ref:` never deletes the directory. `txtodo prune --orphans` lists directories no line points to and deletes them only with `--yes`.
11. **Sync scope.** Every `todo.txt` and `notes.md` under the workspace root, at any depth, is a synced document. Discovery is by walking the tree, not by following tags, so a directory created by hand is picked up too.
12. **Other tools** see an inert tag. `todo.sh -d <ref-dir>/todo.cfg` works on a sub-list like any other file.

### 3.3 Keyboard and accessibility floor (desktop)

- Every action reachable from the keyboard. Line list is a listbox; popover traps focus; detail view is a page, not a modal.
- Colours are never the only signal: completed lines are struck through, refs have an icon, errors have text.

---

## 4. Milestones

Each milestone lists: goal, tasks, acceptance criteria, and what is explicitly out of scope. Acceptance criteria are tests or commands that must pass.

### M0 — Bootstrap (est. 1–2 days)

**Goal.** An empty workspace that builds, tests, lints and fuzzes in CI.

**Tasks.**

- Create the workspace per §2 with empty crates and `lib.rs`/`main.rs` stubs.
- `justfile` with `check` (fmt + clippy + test), `test`, `fuzz <target> <secs>`, `bench`, `corpus` (runs corpus round-trip), `release`.
- GitHub Actions: matrix `ubuntu-latest`, `macos-latest`, `windows-latest`; jobs for `just check`, a 60 s fuzz smoke on Linux, and `cargo deny` (licenses + advisories).
- `docs/adr/0000-template.md`; write ADRs 0001–0012 from §1.
- Copy `specs/todotxt.abnf` from the design doc (§2.3) and add the `ref` value grammar from §3.2.1.
- Seed `corpus/` with at least 30 hand-written lines covering every row of the design doc's edge-case table (§2.4) plus the `ref:` cases. Each corpus file has a sibling `.tokens.json` with the expected token stream (write these by hand; they are the oracle).

**Acceptance.** CI green on all three OSes. `just fuzz parse_line 60` runs (target may be a stub that does nothing yet).

### M1 — `txtodo-core` (est. 1–2 weeks)

**Goal.** A correct, fast, round-trip-preserving parser/formatter/tokenizer with bindings-ready types.

**Public API (freeze this shape; details may grow).**

```rust
pub enum Mode { Strict, Lenient }

pub struct Span { pub kind: TokenKind, pub start: usize, pub end: usize } // byte offsets, UTF-8 safe
pub enum TokenKind { CompletionMarker, CompletionDate, CreationDate, Priority,
                     Project, Context, TagKey, TagValue, IdTag, Url, Text, Whitespace }

pub struct Line<'a> { pub raw: &'a str, pub kind: LineKind<'a>, pub quirks: Quirks, pub ending: LineEnding }
pub enum LineKind<'a> { Blank, Task(Task<'a>) }

pub struct Task<'a> {
    pub completed: bool,
    pub completion_date: Option<Date>,
    pub creation_date: Option<Date>,
    pub priority: Option<Priority>,          // 'A'..='Z'
    pub description: &'a str,                // everything after the prefix, verbatim
}
impl<'a> Task<'a> {
    pub fn projects(&self) -> impl Iterator<Item=&'a str>;
    pub fn contexts(&self) -> impl Iterator<Item=&'a str>;
    pub fn tags(&self) -> impl Iterator<Item=(&'a str, &'a str)>;
    pub fn tag(&self, key: &str) -> Option<&'a str>;   // first match
    pub fn id(&self) -> Option<Ulid>;
    pub fn ref_slug(&self) -> Option<&'a str>;         // validated per §3.2.1
}

pub fn parse_line(raw: &str, mode: Mode) -> Result<Line<'_>, ParseError>;
pub fn tokenize(raw: &str) -> Vec<Span>;               // never fails; unknown → Text
pub fn parse_file(bytes: &[u8]) -> File;               // detects BOM, line endings, trailing newline
pub struct File { pub lines: Vec<OwnedLine>, pub bom: bool, pub ending: LineEnding, pub trailing_newline: bool }
impl File { pub fn to_bytes(&self) -> Vec<u8>; }

// Mutation API: every mutation returns a new OwnedLine; the formatter only rewrites the fields it touched.
pub struct Edit { /* builder: set_priority, clear_priority, complete(today), uncomplete, set_description,
                    set_tag(key, value), remove_tag(key), append(text), prepend(text) */ }
pub fn apply(line: &OwnedLine, edit: &Edit) -> OwnedLine;
pub fn diff_lines(a: &File, b: &File) -> Vec<LineDiff>;         // Myers, by id when present, else by content
pub fn diff_text(a: &str, b: &str) -> Vec<TextEdit>;             // char-level, for description merges
```

**Tasks.**

- Hand-written recursive-descent parser matching `specs/todotxt.abnf`; strict and lenient modes; quirks enum for each leniency (`no_completion_date`, `priority_after_x`, `priority_after_date`, `tabs`, `trailing_ws`, `invalid_ref`, …).
- URL detection before tag detection: schemes `http`, `https`, `mailto`, `ftp`, `ssh`, `file`, `tel`, `sms` — configurable via a `&[&str]` parameter, default list in `core::urls::DEFAULT_SCHEMES`.
- Formatter with field-level dirtiness so untouched bytes are preserved. `complete()` implements the spec's `pri:` rule; `uncomplete()` restores priority from `pri:` and removes the tag.
- `tokenize` shares the parser's scanner; tokens must cover every byte (`Whitespace` included) so highlighters can paint without gaps.
- `File` handles BOM, `\n`/`\r\n` (per-file; mixed endings → quirk, preserved per line), missing trailing newline.
- A second parser generated from the ABNF (`abnf` + `pest`, or `abnf-to-pest`) used only in tests, differentially against the hand-written one on the corpus and on proptest-generated input.
- `proptest` strategies for valid tasks; properties: `format(parse(x)) == x` for corpus lines; `parse(format(t)) == t` for generated tasks; `tokenize` covers `[0, len)` exactly; `apply` then `apply` inverse is identity for priority/complete.
- `cargo-fuzz` target `parse_line` (both modes) and `parse_file`.
- Benchmarks (`criterion`): parse 100 k lines; budget ≤ 150 ms on the CI runner.
- `no_std` + `alloc` feature flag builds (no I/O in this crate anyway).

**Acceptance.**

- `just corpus` passes: every corpus line round-trips byte-for-byte and its token stream equals the `.tokens.json` oracle.
- Differential parser test passes on corpus + 10 000 generated lines.
- `just fuzz parse_line 3600` and `just fuzz parse_file 3600` complete with zero crashes (run once locally; CI runs 60 s).
- Bench within budget.
- Every row of the design doc's edge-case table (§2.4) is a named unit test.

**Out of scope.** Query language, any file I/O beyond `parse_file(bytes)`.

### M2 — `txtodo` CLI with todo.sh parity, direct-file mode (est. 1 week)

**Goal.** A useful tool on day one, before any daemon exists.

**Tasks.**

- `clap` CLI. Commands and aliases exactly as todo.sh: `add`/`a`, `addm`, `list`/`ls`, `listall`/`lsa`, `listpri`/`lsp`, `listproj`/`lsprj`, `listcon`/`lsc`, `listfile`/`lf`, `do`, `pri`/`p`, `depri`/`dp`, `append`/`app`, `prepend`/`prep`, `replace`, `del`/`rm`, `move`/`mv`, `archive`, `deduplicate`, `report`.
- Line numbers as identifiers, exactly like todo.sh. Add `--json` on every listing command (one object per line: `line`, `raw`, parsed fields, `spans`).
- Direct-file mode: read → mutate via `core::apply` → atomic write (temp + rename), honouring §1 rules. `add` stamps the creation date and appends `id:` (config `id_tags = true` default; `--no-id` flag for tests).
- `txtodo fmt` (canonicalise quirks, explicit), `txtodo lint` (report quirks), `txtodo env` (print resolved paths/config).
- Config: `config.toml` with `todo_dir`, `id_tags`, `url_schemes`; env `TXTODO_TODO_DIR`; `--dir` flag.

**Acceptance.**

- Differential harness `tests/todosh_parity.rs`: for each scripted scenario (≥ 25, covering every command), run the same commands through `todo.sh` (vendored at a pinned commit under `tests/vendor/`) and through `txtodo --no-id`, then assert the resulting `todo.txt` is byte-identical (auto-archive-after-`do` suppressed on both sides: this app's own `archive` no longer matches todo.sh's done.txt convention, so it is covered by this crate's own tests instead). Skip on Windows CI if `todo.sh` can't run there; must pass on Linux and macOS.
- `txtodo add` on a CRLF file keeps CRLF; on a file without trailing newline, behaves like todo.sh.

**Out of scope.** Daemon, history, sync.

### M3 — Store, projection and reconciler on one device (est. 2 weeks)

**Goal.** `txtodod` owns the file, records every change, survives external edits, and offers history and undo. Still one device, no CRDT yet — but the op model must already be the one the CRDT will use.

**Op model (`txtodo-model`).**

```rust
pub struct Hlc { pub wall_ms: u64, pub counter: u16, pub device: DeviceId }
pub struct Op { pub id: OpId, pub hlc: Hlc, pub principal: Principal, pub file: FilePath, pub kind: OpKind }
pub enum OpKind {
    Insert { task: TaskId, after: Option<TaskId>, line: String },
    SetField { task: TaskId, field: Field, value: FieldValue },   // completed/dates/priority/quirks/deleted
    EditText { task: TaskId, edits: Vec<TextEdit> },              // description
    Move { task: TaskId, after: Option<TaskId>, to_file: FilePath },
    NotesEdit { file: FilePath, edits: Vec<TextEdit> },
    BlankInsert { after: Option<TaskId> } / BlankRemove { .. },
}
pub enum Principal { User { device: DeviceId }, Agent { token_id: TokenId, name: String, device: DeviceId }, External { device: DeviceId } }
```

**Store (`txtodo-store`).** SQLite schema (write it as `migrations/0001.sql`):

```sql
CREATE TABLE ops (seq INTEGER PRIMARY KEY, op_id BLOB UNIQUE, hlc_wall INTEGER, hlc_counter INTEGER,
                  device BLOB, principal TEXT, file TEXT, kind TEXT, payload BLOB, signature BLOB);
CREATE INDEX ops_file_hlc ON ops(file, hlc_wall, hlc_counter);
CREATE TABLE projections (file TEXT PRIMARY KEY, bytes BLOB, hash BLOB, written_at INTEGER);
CREATE TABLE snapshots (file TEXT, seq INTEGER, state BLOB, PRIMARY KEY(file, seq));
CREATE TABLE meta (key TEXT PRIMARY KEY, value BLOB);   -- device id, keys (encrypted), schema version
```

**Daemon (`txtodo-daemon`).**

> **Superseded by ADR 0025 (M11, 2026-09-13).** This section describes the per-directory daemon as
> originally built (one `txtodod --dir <workspace>`, socket at `<workspace>/.txtodo/txtodod.sock`,
> one service unit per workspace hash) — design §5's own text already called for one process *per
> device*, and M11 realigns the build with it: one `txtodod` per device, a workspace registry
> (`WorkspaceCatalog`/`WorkspaceRegistry`), one socket/service unit total, and every gRPC call
> carrying a `WorkspaceSelector`. The `--dir <workspace>` flag is kept as a bridge — it binds the
> exact locations below unmodified, so the existing single-workspace test suite and the bullets
> here stay accurate for that mode. See design §5's own updated text for the current shape, and
> `crates/txtodo-daemon/CLAUDE.md` for what is and isn't wired up yet.

- One `FileActor` per synced document (todo.txt / notes.md), single writer, owning: in-memory state, projection bytes, projection hash.
- `Watcher` (`notify` crate) with 150 ms debounce; ignore list `*.swp *~ *.tmp .#*`; on event, send `ExternalChange` to the actor.
- Reconciler in the actor (design doc §4.3), implemented without the CRDT for now: state = ordered `Vec<TaskState>`; external edit → `diff_lines` by `id:` → ops → apply → write projection. Lines that arrive without `id:` get one assigned and written back (tagged mode). Our own writes are recognised by projection hash *and* by a short-lived "expected write" token, because some filesystems coalesce events.
- Workspace walker: discover documents per §3.2.11 at startup and on directory create events.
- gRPC server on the socket (`txtodo-proto`): `ListFiles`, `GetFile`, `Watch` (server-stream of changes), `Apply` (batch of intent-level mutations: add/complete/edit/move/delete; the daemon turns them into ops), `History`, `Undo`, `Checkout`. The CLI switches to daemon mode when the socket exists; direct-file mode remains as fallback and for `--no-daemon`.
- Service files: `deploy/launchd/`, `deploy/systemd/`; `txtodo daemon install|start|stop|status` (M11: one unit for the whole device, not one per workspace — `install` migrates any pre-M11 per-workspace units it finds).
- `txtodo log`, `txtodo blame <line>`, `txtodo undo`, `txtodo checkout <iso-datetime> [--stdout]`.
- `txtodo doctor`: socket reachable, watcher alive, file writable, clock sanity, config valid.

**Acceptance (integration tests in `txtodo-daemon/tests/`, each starting a real daemon on a temp dir).**

- External edit scenarios: edit description in place; insert a line in the middle; delete a line; reorder two lines; strip every `id:` tag; append a line with no `id:`; replace the whole file with a copy that has different line endings; write via `todo.sh do 3`. After each: the daemon's state matches the file, exactly one write occurred (if any), unrelated lines are byte-identical, and `txtodo log` shows the expected ops with principal `External`.
- Editor save patterns: vim (write temp + rename), VS Code (truncate + write), `sed -i` (rename). All three reconcile correctly.
- `txtodo undo` after an external edit restores the previous bytes exactly.
- `txtodo checkout` at a timestamp between two ops renders the intermediate state.
- Crash safety: kill -9 the daemon mid-write; on restart the file is either the old or new projection, never partial (atomic rename), and the op log is consistent.

**Out of scope.** Sync, CRDT merge semantics (single device ⇒ no concurrency).

### M4 — CRDT, LAN sync, pairing (est. 3 weeks)

**Goal.** Two devices on the same network converge, offline edits merge, and the conflict table is a passing test suite.

**Tasks.**

- `txtodo-crdt`: Loro document per file with the shape in the design doc §4.2. Map `OpKind` ⇄ Loro ops both ways; the op log stores our `Op`, Loro is the merge engine. Replace M3's `Vec<TaskState>` with the Loro-backed state; keep the reconciler's external interface unchanged.
- HLC implementation with clock-skew guard (reject/warn on > 5 min drift from peers).
- Same-word edit detection: after a merge, if two concurrent `EditText` ops overlap in range, mark the task `needs_review` (stored in the op log as a local flag, not in the file) and expose it via gRPC `Watch`. `txtodo conflicts` lists them; `txtodo conflicts resolve <line> mine|theirs|merged` clears the flag by writing the chosen text as a new op.
- `txtodo-sync` protocol: messages `Hello{device, group, heads}`, `Want{missing ranges}`, `Ops{batch}`, `Ack`. Encoding: `postcard`. Every `Ops` payload encrypted with the group key (XChaCha20-Poly1305) and signed by the device (Ed25519). Version field from day one.
- Transports: LAN via mDNS discovery + iroh QUIC endpoint (iroh's local-discovery; do not enable relay yet).
- Pairing: `txtodo pair` shows a QR + SAS (6 words from the EFF short list) derived from an X25519 handshake; the other device runs `txtodo pair <code>` or scans. Result: both devices hold the group key; new device receives a full snapshot then ops.
- Device removal: `txtodo device remove <id>` rotates the group key; remaining devices re-encrypt nothing (old ops stay under old key, kept for history; new ops use new key).
- Keys in the OS keystore via the `keyring` crate; fall back to an encrypted file with a user passphrase on headless Linux.
- **Sync simulator** (`txtodo-crdt/tests/sim.rs`): N in-process devices, deterministic PRNG, random ops + random external edits + random partitions; after healing, assert all projections byte-identical and every inserted description substring is present somewhere (no-loss).

**Acceptance.**

- `specs/conflicts.md` rows are test cases in `tests/conflicts.rs`; all pass, including "delete vs edit ⇒ resurrect" and "delete vs complete ⇒ completed".
- Simulator: 1 000 runs × 5 devices × 200 ops, zero convergence failures, zero loss.
- Two real daemons on one machine (different ports/dirs) pair over loopback mDNS and converge within 2 s (measured in test).
- A peer cannot join without the group key; a tampered op is rejected (signature test).

**Out of scope.** Relay, internet, mobile.

### M5 — `ref:` directories and notes (est. 1 week)

**Goal.** Everything in §3.2, end to end, in the daemon and CLI.

**Tasks.**

- Ref slug validation in `core` (M1 already parses it; add the `invalid_ref` quirk if missing).
- Workspace tree model in `txtodo-model`: a file's parent line, children discovery, progress computation (§3.2.5) cached and invalidated on ops.
- Lazy creation, slug generation and collision rule, rename with atomic directory move + tag rewrite in one op batch.
- `notes.md` as a Loro text document; `NotesEdit` ops; exposed via gRPC `GetNotes`/`EditNotes`.
- Move-line-between-files moves the directory (§3.2.8) with rollback.
- `txtodo open <line>` prints the ref path; `txtodo notes <line>` opens `$EDITOR` on `notes.md` (creating lazily); `txtodo sub <line>` runs any `txtodo` command scoped to the sub-list (`txtodo sub 2 ls`); `txtodo prune --orphans [--yes]`.
- gRPC `ListFiles` returns the tree with progress; `Watch` emits progress changes.

**Acceptance.**

- Creating notes on a line with no `ref:` produces exactly one op batch that adds the tag and creates the directory; the parent file changes only on that one line.
- Progress numbers match §3.2.5 for a fixture tree three levels deep.
- Archiving a parent keeps the tag and the directory; deleting keeps the directory; `prune --orphans` finds it.
- Syncing a workspace with nested refs to a fresh device reproduces the whole tree.
- `todo.sh -d <ref>/todo.cfg ls` lists the sub-list.

### M6 — MCP server and agent tokens (est. 2 weeks)

**Goal.** Any MCP client can work the list with scoped, attributed, dry-runnable access.

**Tasks.**

- `txtodo-mcp` using `rmcp`: tools, resources, prompts exactly as the design doc §6.3–6.4, plus `todo_notes_get {id}` / `todo_notes_set {id, text}` and `file` parameters accepting a ref path (`q4-roadmap/todo.txt`).
- Transports: stdio (`txtodo mcp --stdio`) and Streamable HTTP on `127.0.0.1:8636/mcp`, loopback only, with a `Host` and `Origin` check (403 for a foreign one). There is no `--lan` and no mDNS advertisement (decided 2026-09-20, `tasks/mcp-local-only`).
- Tokens: macaroon-style (`macaroon` crate or a minimal HMAC-chained implementation in `txtodo-mcp::token`): root secret in the keystore; caveats `scope=…`, `project=…`, `context=…`, `file=…`, `expires=…`, `quarantine=@ctx`. `txtodo token create|list|revoke|attenuate`. Revocation list in the store.
- Bearer auth on HTTP; stdio inherits a token from `--token` or the config's `default_stdio_token`.
- Every mutation passes through the daemon's `Apply` with `Principal::Agent`. Quarantine caveat appends the context to `todo_add` lines. Rate limit: 60 mutations/min/token, 10 deletes/min/token, exceeding ⇒ token paused + notification event.
- `dry_run` on `todo_batch` returns a unified diff against the projection.
- Structured errors: `{ code, message, line?, spec_rule? }`.
- Resource subscriptions wired to the daemon's `Watch`.

**Acceptance.**

- Scope matrix test: for each (scope set × tool) pair, the expected allow/deny; attenuated tokens can't widen.
- Quarantined `todo_add` produces a line with the extra context and `txtodo blame` shows the agent principal.
- Dry run leaves the file's hash unchanged.
- Rate-limit test pauses a token and emits the event.
- Smoke test with a real MCP client (the SDK's reference client) over both transports.

### M7 — Desktop app (est. 3–4 weeks)

**Goal.** The interactive text file, per §3.1–3.3, on macOS/Windows/Linux.

**Tasks.**

- Tauri 2 shell; Rust side talks gRPC to `txtodod` (spawns it if absent).
- Lezer grammar for todo.txt generated from `specs/todotxt.abnf` (script in `apps/desktop/scripts/abnf-to-lezer.mjs`; check the generated grammar in and add a CI step that fails if regeneration differs). Highlight tags map to the semantic token names in §3.1.
- Main view: read-only CM6 `EditorView` bound to the daemon's `Watch` stream; decorations for hidden `id:` tags, ref indicators, hover pencil, "Add a line" trailing row.
- Edit popover: a floating single-line CM6 instance with the same language; chips per §3.2; strict-mode validation via the WASM build of `core` (`txtodo-ffi` wasm target) so the UI never re-implements the grammar.
- Detail view: pinned parent, notes editor (CM6 markdown mode), recursive file view component, breadcrumb, footer.
- Conflict banner + review sheet when `needs_review` arrives (three variants from M4).
- Devices and agents screen: pair (QR render + scan via webcam), token create/revoke, activity feed from the op log.
- Menu-bar quick-add (global hotkey) that opens only the popover editor.
- Stretch: raw mode (`Cmd/Ctrl+E`).

**Acceptance.**

- Playwright/WebDriver tests: click opens popover with the raw line including `id:`; Enter saves and the file on disk changes only that line; double-click opens detail; typing into empty notes creates the directory; sub-list line double-click nests the breadcrumb; conflict sheet appears when the test injects concurrent ops via a second daemon.
- Visual regression snapshots for light and dark themes.
- Startup to first paint of a 10 k-line file ≤ 500 ms on the CI runner.

### M8 — Relay, internet sync, push (est. 2 weeks)

**Tasks.** Enable iroh relay + hole-punching; reference relay binary in `relay/` (stores ciphertext blobs keyed by group + device, forwards APNs/FCM wake-ups when M9 registers tokens); file-carrier transport (`sync/<device-id>.ops` append-only files under a user-chosen folder); `txtodo bundle export|import`. Relay is optional; self-hosting docs.

**Acceptance.** Two daemons on separate networks (simulate with network namespaces in CI, or a two-VM job) converge via relay within 30 s; via direct hole-punch when possible. File-carrier: two daemons sharing a directory (no network) converge after each writes its ops file.

### M9 — Android, then iOS (est. 4–6 weeks)

**Tasks.** `txtodo-ffi` uniffi bindings for core + a `DaemonHandle` API that embeds `txtodod` in-process. Android: foreground service running the daemon, Compose UI implementing §3.1–3.2 with `AnnotatedString` from `tokenize`, MCP over HTTP on LAN while the service runs, FCM wake-ups. iOS: SwiftUI + `UITextView`, daemon embedded in the app process, `BGAppRefresh`, silent push, App Intents for add/complete/list, share sheet. Both: QR pairing, widgets.

**Acceptance.** Shared UI test script (Maestro or equivalent) runs the same scenario as M7's Playwright suite. A phone and a desktop pair and converge on LAN in ≤ 2 s and via relay in ≤ 30 s.

### M10 — Everything else (ongoing)

~~Sidecar identity mode~~ (built early, now the default — decision 9, `docs/questions.md` Q2); web PWA; TUI; editor plugins (Neovim, VS Code, Obsidian) that use the LSP-style `tokenize` over the socket; WASM plugin host; Prometheus + OTel; TLA+ spec and TLC run in CI; Nix flake; signed releases; SBOM. Each item gets its own mini-plan when started.

---

## 5. Cross-cutting requirements

**Performance budgets** (fail CI if exceeded, measured on the Linux runner): parse 100 k lines ≤ 150 ms; reconcile a single external line edit in a 10 k-line file ≤ 20 ms; sync 1 000 ops between two loopback daemons ≤ 500 ms; daemon idle RSS ≤ 50 MB with a 10 k-line workspace.

**Security checklist** (review before M4, M6, M8, M11 close): no secrets in logs; keys only in keystore; every network message versioned, authenticated, encrypted; MCP HTTP never binds a non-loopback address and refuses a foreign `Host` or `Origin`; tokens never logged; path traversal impossible via `ref:` (fuzz the slug validator); relay cannot distinguish op types; content a paired device pushes stays inert and contained (M11: an auto-accepted Remote workspace lands only under the data dir's `remote/<workspace-id>/`, is plain text never run, carries no absolute path, and a removed one never comes back on its own; the default workspace merges only with a device both humans called their own, ADR 0029 amendment).

**Observability from M3 onward:** `tracing` spans `reconcile{file}`, `sync.session{peer}`, `mcp.call{tool,principal}`; JSON logs to `.txtodo/logs/` with rotation; `txtodo doctor --verbose` dumps the last 100 events.

**Docs:** each milestone updates `README.md` (user-facing) and `docs/` (contributor-facing). The `specs/` files are normative and must be updated in the same PR as any behaviour they describe.

---

## 6. Open questions to escalate (answer in `docs/questions.md`)

1. Should files other than the three managed ones inside a `ref:` directory ever sync (attachments)? Default until answered: no.
2. Sidecar mode's confidence threshold and cost weights (M10).
3. Whether `rec:` recurrence is a core feature or stays a plugin (design doc says plugin; M10).
4. iOS: is a Local Network permission prompt acceptable for LAN MCP, or should iOS be relay-only for agents?
5. Relay hosting: does the project run a public relay, or self-host only?

---

## Appendix — first week, concretely

1. M0 in full.
2. M1: parser + tokenizer + `File` with corpus round-trip green; formatter and `Edit` next; proptest and fuzz last.
3. Open PRs small: one per bullet in the task lists. A 2 000-line PR will be sent back.
