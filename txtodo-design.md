# txtodo — the most over-engineered todo.txt app

> One must imagine the todo list complete.

txtodo is a todo.txt app built like a distributed database, because your todo list deserves the same convergence guarantees as a bank ledger. It is cross-platform (macOS, Linux, Windows, iOS, Android, web, terminal), cross-device (your phone, laptop, watch and the Raspberry Pi under the stairs form one *device group*), syncs peer-to-peer with end-to-end encryption and no required server, and runs an MCP server on every device so agents can work your list from wherever they happen to be running.

And it is still just a text file. `cat todo.txt` works. `vim todo.txt` works. `todo.sh` works on the same file at the same time.

A note on the brief: "MVP server" is read here as **MCP** (Model Context Protocol), since that's what agents speak. The daemon also exposes a plain REST API, so if a minimum-viable HTTP server was the intent, that's covered too.

---

## 0. Design goals

| Goal | What it means in practice |
|---|---|
| **The file is the truth** | `todo.txt` (and `done.txt`) is the canonical state. Every other structure in this document is a cache, index, or projection of it. If txtodo vanished tomorrow, you'd lose nothing. |
| **Faithful to the one-line standard** | The app never writes a line that isn't valid todo.txt. App metadata lives only in `key:value` tags, which the spec explicitly permits, and the app still works if those tags are stripped. |
| **Cross-platform** | One Rust core, compiled to native, WASM, Swift, Kotlin, C, Python and Node. Thin native shells per platform. |
| **Cross-device, no server required** | Devices form an encrypted peer group. Sync works over LAN, over the internet with hole-punching, through an optional dumb relay, over Bluetooth, over your existing Dropbox/Syncthing folder, or over a USB stick. |
| **Agents are first-class** | Every device runs an MCP server (stdio + HTTP) with scoped capability tokens, attribution, dry-run, and audit. |
| **Over-engineered on purpose** | Every layer has to justify itself by naming the failure it prevents. Over-engineering is allowed; *pointless* engineering is not. |

---

## 1. Architecture at a glance

```
                    ┌──────────────────────────────────────────┐
                    │              CLIENTS (per device)         │
                    │  txtodo CLI · TUI · Tauri desktop · iOS ·    │
                    │  Android · Web PWA · nvim/VS Code/Emacs · │
                    │  widgets · Shortcuts · Raycast            │
                    └───────────────┬──────────────────────────┘
                                    │ local IPC (unix socket / named pipe / gRPC)
   agents ──MCP (stdio|http)──▶ ┌───┴──────────────────────────────┐
   curl   ──REST/OpenAPI────▶  │         txtodod  (per-device daemon) │
   Grafana──/metrics────────▶  │  file watcher · reconciler ·      │
                               │  CRDT doc · op log · sync engine  │
                               │  MCP server · REST · plugins      │
                               └───┬───────────────────┬──────────┘
                                   │                   │
                     ┌─────────────┴────────┐    ┌─────┴──────────────────────┐
                     │  todo.txt / done.txt │    │  peers (other devices)     │
                     │  THE SOURCE OF TRUTH │    │  mDNS+QUIC · hole-punch ·  │
                     │                      │    │  relay · BLE · file-carrier│
                     └──────────────────────┘    └────────────────────────────┘
```

Everything that touches a line of text goes through `txtodo-core`, the Rust library that every client and the daemon share.

---

## 2. Layer 0 — the file, and the faithfulness contract

### 2.1 What the spec says (the whole thing)

```
(A) 2026-09-11 Call the plumber +house @phone due:2026-09-15
x 2026-09-11 2026-09-01 Renew passport +admin
```

- One task per line. Plain UTF-8 text.
- **Priority** `(A)`–`(Z)`: uppercase, in parentheses, followed by a space, always first if present.
- **Creation date** `YYYY-MM-DD`, optionally after the priority.
- **Completion**: the line starts with lowercase `x` and a space, then the **completion date**, then the creation date if there was one.
- **Projects** `+word` and **contexts** `@word` anywhere in the description, preceded by a space.
- **Metadata** as `key:value`, no spaces.
- The spec notes many clients drop priority on completion, and suggests `pri:A` to preserve it.
- Convention (from todo.sh): completed tasks are archived to `done.txt`.

That's it. The rest of this document exists to serve those eight bullets.

### 2.2 The contract (seven rules the codebase is not allowed to break)

1. **Canonical file.** All state is derived from `todo.txt` + `done.txt`. Indexes, CRDT state, history and caches can be deleted and rebuilt from the files. The reverse is never true.
2. **Byte-preserving round trip.** A line txtodo didn't change is written back byte-for-byte: same whitespace, same tag order, same quirks. `parse ∘ format = id` and `format ∘ parse = id` are property tests over every line in the compatibility corpus.
3. **Spec-only output.** txtodo never emits a construct the spec doesn't define. It writes `pri:A` on completion rather than inventing its own priority placement.
4. **Metadata only in tags.** Every app-specific datum is a `key:value` tag, documented in Appendix A. The app degrades gracefully, never breaks, when tags are absent.
5. **Order is data.** Line order is preserved and synced, because `todo.sh` (and humans) identify tasks by line number.
6. **Blank lines are entries.** They're preserved as void entries so line numbers stay stable for other tools.
7. **File hygiene is preserved, not imposed.** Line endings (LF/CRLF), a BOM, trailing newline or lack of one: detected per file and kept. Canonicalisation is an explicit command (`txtodo fmt`), never a side effect.

### 2.3 Grammar

Strict grammar (`specs/todotxt.abnf`), which the parser's strict mode enforces and the formatter always emits:

```abnf
line        = completed / incomplete / blank
incomplete  = [priority SP] [date SP] description
completed   = "x" SP date SP [date SP] description   ; completion date, then creation date
priority    = "(" %x41-5A ")"                        ; (A) through (Z)
date        = 4DIGIT "-" 2DIGIT "-" 2DIGIT
description = word *(SP word)
word        = project / context / tag / plain
project     = "+" 1*NONSP
context     = "@" 1*NONSP
tag         = key ":" 1*NONSP
key         = 1*(%x21-39 / %x3B-7E / %x80-10FFFF)   ; any non-space, non-colon
plain       = 1*NONSP
NONSP       = %x21-7E / %x80-10FFFF                  ; anything except whitespace
blank       = ""
```

**Lenient mode** (default when *reading*, never when writing) additionally accepts what real files contain:

- `x` with no completion date
- priority kept on completed lines, in either position (`x (A) 2026-…` or `x 2026-… (A) …`)
- tabs, runs of spaces, trailing whitespace (all preserved)
- `+` or `@` at the very start of the line

Each leniency is recorded on the parsed task as a *quirk* so it round-trips (rule 2). `txtodo lint` reports quirks; `txtodo fmt` fixes them on request.

### 2.4 Edge cases the parser gets right

| Input | Behaviour | Why |
|---|---|---|
| `mail bob@example.com` | not a context | `@` must be preceded by a space |
| `see https://example.com/x` | not a tag | URL schemes are detected before tag parsing; the scheme list is configurable |
| `learn C++ +cpp` | one project, `+cpp` | `C++` has no space before its `+` |
| `买菜 +家务 @手机` | project `+家务`, context `@手机` | NONSP is Unicode, not ASCII |
| `X 2026-09-11 not done` | incomplete task whose text starts with "X" | the completion marker is lowercase only |
| `x 2026-09-11 (A) task` | completed, quirk `priority_after_x` | lenient read, never written |
| `note: buy milk` | plain word `note:` | a tag needs a non-empty value with no space |
| `(a) task` | plain text | priority is uppercase only |
| line ending in `\r\n` | preserved | rule 7 |

### 2.5 Extension tags

All optional, all `key:value`, all borrowed from existing conventions where one exists:

| Tag | Meaning | Origin |
|---|---|---|
| `id:01J9K…` | stable task identity (ULID) — see §4.1 | txtodo (key name configurable) |
| `pri:A` | priority preserved through completion | todo.txt spec |
| `due:2026-09-15` | due date | widespread convention |
| `t:2026-09-14` | threshold: don't show before this date | SimpleTask convention |
| `rec:1w` / `rec:+1w` | recurrence, from completion / strictly from due | SimpleTask convention |
| `h:1` | hidden | SimpleTask convention |
| `ref:q4-roadmap` | a sibling directory holding this task's notes and sub-list — see §2.6 | txtodo |

Nothing else. Attribution ("which agent added this"), history, and sync state live in the op log, not in the file.

### 2.6 Detail on disk: the `ref:` directory convention

A task that needs more than one line gets a directory, not a richer line.

- `ref:<slug>` names a directory beside the file containing the line (`~/todo/todo.txt` + `ref:q4-roadmap` → `~/todo/q4-roadmap/`). Slug: `[a-z0-9][a-z0-9._-]*`, max 64 chars, no `/`, no traversal.
- The directory may hold `todo.txt`, `done.txt` and `notes.md`. All optional; the sub-list is a full todo.txt file, so it highlights, archives, syncs and nests exactly like the top level.
- Creation is lazy: the tag is added and the directory created on the first keystroke into either notes or sub-list. Slug defaults to kebab-case of the description; collisions get `-2`, `-3`.
- Progress on the parent line = open/total across `<ref>/todo.txt` and `<ref>/done.txt`. Completing the parent is never automatic; the UI offers it when nothing is open.
- Archiving or deleting the parent leaves the directory alone. `txtodo prune --orphans` is the only thing that removes one.
- Every `todo.txt`, `done.txt` and `notes.md` under the workspace root, at any depth, is a synced document, discovered by walking the tree rather than by following tags.
- Other tools see an inert tag; `todo.sh` pointed at the sub-directory works on the sub-list.

The full normative version, including the rename and move-between-files rules, is §3.2 of `txtodo-implementation-plan.md`.

---

## 3. Layer 1 — `txtodo-core`

A `no_std`-compatible Rust crate. No I/O, no clocks, no allocation on the parser hot path.

```
txtodo-core/
  grammar.rs   hand-written recursive-descent parser; a second parser is generated
               mechanically from the ABNF and differentially tested against it in CI
  model.rs     Task { completed, completion_date, creation_date, priority,
                      description: Text, quirks }
               projects / contexts / tags are *views* computed from description on demand
  format.rs    byte-preserving formatter; only touches fields that changed
  diff.rs      line-level (Myers) and description-level (char/word) diff
  corpus/      every weird line ever seen in the wild, with its expected parse
```

**Key modelling decision:** the structured fields are exactly the spec's prefix (`x`, dates, priority). The description is a single string, and projects, contexts and tags are parsed *out of it* as views. This is what makes the CRDT in §4 faithful: it syncs precisely the structure the spec defines, and nothing more.

**Bindings** (`crates/txtodo-ffi`):

- Swift and Kotlin via `uniffi`
- WASM via `wasm-bindgen` (web app, Obsidian plugin)
- C ABI via `cbindgen` for everything else
- Python via `pyo3` and Node via `napi-rs`, because someone will want to script it

**Verification:**

- `proptest`: generate random valid tasks; assert both round-trip identities
- `cargo-fuzz`: the parser never panics on any byte sequence
- Differential tests: run the same command sequence through `txtodo` and `todo.sh`, diff the resulting files, expect zero differences
- Miri on the zero-copy paths

---

## 4. Layer 2 — sync

### 4.1 The identity problem

todo.txt lines have no IDs. Line 7 on your laptop and line 7 on your phone are only the same task if nobody inserted a line above. Sync needs stable identity. Two modes:

**Tagged mode (default).** Every task carries `id:<ULID>` (26 chars, sortable by creation time). Spec-compliant metadata; other tools ignore it. Robust under any combination of concurrent edits. Cost: about 30 bytes of visual noise per line, which clients hide by default.

**Sidecar mode (purist).** No tags in the file. IDs live in `.txtodo/index` on each device, keyed by a *fingerprint*. After an external edit, lines are re-identified by solving an assignment problem: cost = weighted mix of creation-date equality, project/context overlap, normalised Levenshtein distance on the description, and distance from the previous line position; solved with the Hungarian algorithm; matches below a confidence threshold become delete+insert. Honest caveat: in sidecar mode, *simultaneous* external edits to the same task on two devices may resurrect an edit as a duplicate rather than merge it. It will never lose text.

The `id:` key name is configurable (`sid:`, `uid:`, …) to avoid colliding with tools that use `id:` for something else.

### 4.2 The CRDT document

Built on a Rust CRDT library with map, list and text types (Loro or Automerge; Loro has a native movable list, with Automerge a move is delete+insert). The design is library-agnostic.

```
Doc
├── files: Map<"todo.txt" | "done.txt" | …, MovableList<TaskId>>   line order per file
└── tasks: Map<TaskId, Task>
      ├── completed        LWW<bool>
      ├── completion_date  LWW<Option<Date>>
      ├── creation_date    LWW<Option<Date>>
      ├── priority         LWW<Option<A..Z>>
      ├── description      Text   (character-level merge)
      ├── quirks           LWW<Quirks>
      └── deleted          LWW<bool>   (tombstone)
```

- LWW registers use **hybrid logical clocks** (wall clock + counter + device id), so "last" is meaningful across devices with skewed clocks and ties resolve deterministically.
- `description` is a text CRDT: two devices editing different words of the same task both win. Editing the *same* word produces a character-level interleave; the reconciler flags it and clients show a one-tap "keep mine / keep theirs / keep merged".
- Archiving to `done.txt` is a move between lists plus `completed = true`.
- Multiple todo files ("workspaces": `work.txt`, `home.txt`) are just more entries in `files`.

### 4.3 File ⇄ CRDT reconciliation (the hard part)

The file is the truth; the CRDT is how the truth travels. Both directions must be lossless.

**File → CRDT** (an external edit: vim, Dropbox, `todo.sh`, a stray `sed`):

1. Watcher fires (`notify` crate over FSEvents / inotify / ReadDirectoryChangesW), debounced 150 ms, ignoring `.swp`, `*~`, `.tmp` and partial writes.
2. Hash the file. If it equals the hash of our own last write, ignore.
3. Parse into `L_new`. Load `L_old` (the exact bytes we last wrote) and the current CRDT state `S`, which may already be *ahead* of `L_old` because a peer's ops arrived.
4. Match `L_new` ↔ `L_old` by `id:` tag (tagged mode) or fingerprint assignment (sidecar mode).
5. Derive ops from each match: changed prefix fields → LWW sets; changed description → char diff → text ops; unmatched new line → insert; missing line → tombstone; changed position → move.
6. Apply those ops **on top of `S`** (three-way, not overwrite), so the peer's change and the vim edit both survive.
7. Materialise `S` back to bytes, write atomically (temp file + rename), record the new hash as "our write".

**CRDT → file** (a peer's ops arrived): materialise, and rewrite only if the bytes differ. Untouched lines stay byte-identical (rule 2).

**Concurrency inside the daemon:** one reconciler actor per file, single writer, so the file is never written by two paths at once. Client mutations (CLI, MCP, UI) go through the same actor as CRDT ops, never straight to disk.

### 4.4 Storage

- Append-only op log in SQLite (`.txtodo/oplog.db`), one row per op: `(hlc, device_id, principal, op_bytes, signature)`.
- Snapshots every N ops; compaction keeps snapshots plus ops since. Full history is retained by default (it's a todo list; it's small), with configurable retention.
- Everything under `.txtodo/` is rebuildable from the files. Deleting it is the nuclear reset.

### 4.5 Transports

The sync engine speaks one protocol (`txtodo-sync`: authenticated op exchange with vector-clock "what do you have that I don't") over whatever carrier is available, trying them in order:

| Carrier | When | How |
|---|---|---|
| **LAN** | same network | mDNS/DNS-SD (`_txtodo._udp`) discovery, QUIC transport |
| **Direct internet** | different networks | QUIC with NAT hole-punching (iroh-style), relay-assisted rendezvous |
| **Relay** | hole-punching fails, or a device is asleep | stores *encrypted* op blobs per device group and forwards push wake-ups (APNs/FCM) to mobile. The reference relay is a single Rust binary; any S3 or WebDAV endpoint also works as a dumb relay |
| **Bluetooth LE** | two phones, no network | same protocol, chunked over GATT |
| **File carrier** | you already run Syncthing / Dropbox / iCloud Drive | each device appends ops only to *its own* file in a shared folder (`sync/<device-id>.ops`), so dumb file sync never conflicts; devices ingest each other's files |
| **Sneakernet** | air-gapped | `txtodo bundle export` / `txtodo bundle import`, git-bundle style |

All carriers are optional. LAN alone is a complete system.

### 4.6 Trust and encryption

- Each device has an Ed25519 identity key and an X25519 agreement key, generated on first run and stored in the platform keystore (Keychain, Android Keystore, Secret Service, DPAPI).
- **Pairing** by scanning a QR code or comparing a short authentication string shown on both screens. Pairing admits a device to the *device group*.
- The group has a symmetric key; ops are encrypted with it and signed by the originating device. Removing a device rotates the group key, so removed devices can't read new ops.
- The relay, the file-carrier folder, and any network in between only ever see ciphertext and routing metadata.
- Threat model, explicitly: the relay is untrusted; LAN peers are authenticated by device key; a stolen unlocked device is out of scope (that's the OS's job).

### 4.7 Conflict semantics

The user-visible guarantee: **txtodo never silently loses something you typed.**

| Device A | Device B | Result |
|---|---|---|
| complete | complete | complete (idempotent) |
| complete | edit description | completed, with the edit |
| edit word 1 | edit word 5 | both edits |
| edit word 3 | edit word 3 | char-level merge, flagged for a one-tap review |
| set `(A)` | set `(B)` | LWW by HLC; the loser is visible in `txtodo log` |
| delete | edit | edit wins, task resurrected (default; configurable) |
| delete | complete | completed, not deleted (default; configurable) |
| move up | move down | both moves apply deterministically; same result on every device |
| strip all `id:` tags in vim (tagged mode) or any external edit (sidecar mode, the default since 2026-09-13, `docs/questions.md` Q2) | anything | fingerprint re-identification (§4.1); a full description rewrite becomes a visible duplicate (delete+insert), never a silent merge |
| archive to `done.txt` | edit | the edit lands in `done.txt` |

### 4.8 History

Because the op log is append-only and signed:

- `txtodo log` — every change, with device and principal (`you@laptop`, `agent:claude-code@laptop`)
- `txtodo blame` — per line, who last touched each field
- `txtodo checkout 2026-09-01T09:00` — render the file as it was, to stdout or a temp path
- `txtodo undo` — inverse ops, which themselves sync
- Time travel is a *view*; the on-disk `todo.txt` is always "now".

---

## 5. Layer 3 — `txtodod`, the per-device daemon

One process per device, owning the file(s), the CRDT, the op log, sync, and the API surfaces. Clients never touch the file directly.

| Platform | How it runs | Caveats |
|---|---|---|
| macOS | `launchd` user agent | — |
| Linux | `systemd --user` unit | — |
| Windows | per-user service or startup task | named pipes instead of unix sockets |
| Android | foreground service (persistent notification), `WorkManager` for periodic sync | MCP reachable on LAN while the service runs |
| iOS | embedded in the app as a library; no separate process | MCP over HTTP only while the app is foregrounded (plus the background grace period); background sync via `BGAppRefresh` and silent push from the relay; **App Intents** expose the same operations to Shortcuts and Siri as the always-available agent surface |
| Web | runs in a Web Worker (WASM), storage in OPFS | syncs via WebSocket to a relay or to a LAN daemon |

Local IPC: unix domain socket or named pipe carrying gRPC (`tonic`), with a JSON-over-HTTP mirror on loopback for scripts.

`txtodo doctor` checks: file permissions, watcher health, clock skew vs peers, keystore access, port availability, relay reachability, and whether some other tool is holding the file open.

---

## 6. The agent surface — MCP on every device

### 6.1 Transports

- **stdio**: `txtodo mcp --stdio`, for local agents (Claude Code, Cursor, an editor plugin) that spawn a subprocess.
- **Streamable HTTP**: `http://127.0.0.1:8636/mcp` (8636 spells TODO on a phone keypad). Optionally bound to the LAN and advertised over mDNS as `_txtodo-mcp._tcp`, so an agent on your desktop can find the daemon on your phone.
- Both are the same server; the daemon is the only thing behind them.

### 6.2 Auth: scoped capability tokens

```
txtodo token create --name "claude-code" --scope read,write:add,write:complete --project +work --expires 30d
```

Tokens are macaroon-style: a signed root with attenuating caveats, so a token holder can mint a *narrower* token (hand a sub-agent read-only on `+work`) but never a broader one. Scopes:

| Scope | Allows |
|---|---|
| `read` | list, search, resources, subscriptions |
| `write:add` | append new tasks |
| `write:complete` | complete / uncomplete |
| `write:edit` | edit description, priority, dates |
| `write:delete` | delete / archive (requires `confirm: true` in the call) |
| `raw` | line-level read/write; bypasses structure; for tools that speak todo.txt natively |
| `project:+x`, `context:@y`, `file:work.txt` | restrict every other scope to matching tasks |

Every mutation is recorded in the op log with the token's principal, so `txtodo blame` can tell you which agent added "buy 400 rubber ducks".

### 6.3 Tools

| Tool | Args | Notes |
|---|---|---|
| `todo_list` | `query`, `file`, `limit` | query language from §8; returns tasks with `id`, `line` (current line number), parsed fields, and `raw` |
| `todo_search` | `text` | full-text search over a `tantivy` index |
| `todo_get` | `id` or `line` | single task |
| `todo_add` | `text`, `file` | `text` is a raw todo.txt line minus dates; the daemon stamps the creation date and `id:` |
| `todo_complete` / `todo_uncomplete` | `id` | writes `pri:` preservation per spec |
| `todo_edit` | `id`, `patch` | field-level patch (`priority`, `due`, `append`, `replace`) so agents don't have to reproduce whole lines |
| `todo_move` | `id`, `before` / `after` | reorder |
| `todo_delete` | `id`, `confirm` | tombstone |
| `todo_archive` | `file` | move completed tasks to `done.txt` |
| `todo_batch` | `ops[]`, `dry_run` | atomic; `dry_run` returns the unified diff of the file without applying |
| `todo_history` | `since`, `id` | reads the op log |
| `todo_raw` | `file`, `lines[]` (read) / `line`, `text` (write) | needs the `raw` scope |

Structured errors carry the offending line and a pointer to the spec rule, so an agent that produces `(a) task` learns why.

### 6.4 Resources and prompts

Resources (subscribable; the daemon pushes `notifications/resources/updated` on change):

- `todotxt://todo.txt`, `todotxt://done.txt` — the files, as text
- `todotxt://task/{id}`
- `todotxt://project/{name}`, `todotxt://context/{name}` — filtered views
- `todotxt://history?since=…`

Prompts: `plan_today`, `weekly_review`, `triage_inbox` — parameterised templates that pull the relevant resources in.

### 6.5 Guardrails

- Optional **quarantine**: agent-created tasks get a configurable context (default `@inbox`) so a human triages them; the setting is per token.
- Rate limits per token; a token that deletes more than N tasks a minute is paused and the user notified.
- `dry_run` everywhere; destructive tools require `confirm: true`.
- On mobile, the notification "claude-code added 3 tasks to +work" is on by default.

### 6.6 Example session

```
→ todo_list {"query": "+work and not done and due <= today+3d"}
← [{"id":"01J9K3…","line":4,"raw":"(B) 2026-09-08 Draft Q4 roadmap +work due:2026-09-12","priority":"B",…}]

→ todo_batch {"dry_run": true, "ops": [
     {"todo_edit": {"id":"01J9K3…", "patch": {"priority":"A"}}},
     {"todo_add":  {"text":"Review roadmap draft +work @laptop due:2026-09-14"}}]}
← {"diff": "--- todo.txt\n+++ todo.txt\n@@ -4 +4,5 @@\n-(B) 2026-09-08 Draft Q4 roadmap +work due:2026-09-12 id:01J9K3…\n+(A) 2026-09-08 Draft Q4 roadmap +work due:2026-09-12 id:01J9K3…\n+2026-09-11 Review roadmap draft +work @laptop due:2026-09-14 id:01J9M7…"}

→ todo_batch {"dry_run": false, "ops": [ …same… ]}
← {"applied": 2, "hlc": "…"}
```

Two seconds later the same two lines are on your phone.

---

## 7. Clients

All clients are thin: they talk to `txtodod` over local IPC and render. None of them parse the file themselves.

**The UI model is the file.** Every graphical client renders `todo.txt` as a syntax-highlighted document with real line numbers, using `txtodo_core::tokenize` so token boundaries are identical on every platform. `id:` tags are hidden by default with a toggle. A single click or tap on a line opens a popover that edits the raw line (with chips that insert or toggle tokens); a double click or tap opens the line's `ref:` directory as a detail view: the parent line pinned at the top, `notes.md` as a plain markdown editor, and the sub-list rendered by the same file-view component, recursively. The precise interaction spec is §3 of `txtodo-implementation-plan.md`.

| Client | Stack | Notable |
|---|---|---|
| `txtodo` CLI | Rust, `clap` | **todo.sh command-compatible** (`add`, `ls`, `do`, `pri`, `depri`, `append`, `prepend`, `replace`, `del`, `archive`, `listproj`, `listcon`) so existing aliases and muscle memory keep working; plus `log`, `blame`, `checkout`, `undo`, `sync`, `pair`, `token`, `mcp`, `doctor`, `fmt`, `lint`; `--json` on everything |
| TUI | `ratatui` | vim keys, live sync indicator, conflict review |
| Desktop | Tauri 2 shell, Svelte UI | macOS / Windows / Linux; menu-bar quick-add; global hotkey |
| iOS | SwiftUI + uniffi core | widgets, Live Activities for tasks due today, App Intents / Shortcuts, watchOS complication, Share Sheet |
| Android | Compose + uniffi core | widgets, Quick Settings tile, foreground-service daemon |
| Web PWA | WASM core in a Web Worker, OPFS | installable, offline, syncs via relay |
| Editors | Neovim (Lua), VS Code, Emacs, Obsidian | syntax highlighting generated from the same ABNF, completions for `+project` / `@context`, inline conflict markers |
| Launchers | Raycast, Alfred, GNOME Shell, PowerToys Run | quick-add and quick-complete |

---

## 8. Query language

One grammar shared by the CLI, MCP, and every UI's filter bar:

```
txtodo ls '+work and @phone and not done'
txtodo ls 'pri >= B and (due < today+7d or due is none)'
txtodo ls 'created > 2026-09-01 and text ~ "roadmap"'
txtodo ls 'done and completed >= start_of_week'
```

- Operands: `done`, `pri`, `due`, `t`, `created`, `completed`, `+project`, `@context`, `key:value`, `text`, `line`, `id`
- Relative dates (`today`, `+3d`, `start_of_week`, `eom`); natural-language dates via a plugin
- Compiles to a plan over the in-memory index; `txtodo ls --explain` prints the plan, because of course it does
- Saved views (`txtodo view save today '…'`) sync like everything else

---

## 9. Plugins

WASM Component Model (WASI 0.2), sandboxed, capability-gated: a plugin declares which hooks it needs and gets nothing else.

Hooks: `on_parse_line`, `before_write`, `on_task_added`, `on_task_completed`, `query_function`, `view_filter`, `format_line`.

Ships with: recurrence (`rec:`), ICS calendar export for `due:`, natural-language dates, a GitHub issue mirror (`gh:owner/repo#123` ↔ task), and "notify when something is due today". Plugins run inside `txtodod` and install from a registry or a local path.

---

## 10. Observability

Because you should be able to page yourself when your todo list falls behind.

- `txtodod` exposes Prometheus at `/metrics`: `txtodo_tasks{file,state,project}`, `txtodo_sync_lag_seconds{peer}`, `txtodo_reconcile_total{outcome}`, `txtodo_conflicts_flagged_total`, `txtodo_mcp_calls_total{tool,principal}`.
- OpenTelemetry traces: one trace per sync session and per reconciliation, so you can see exactly why your phone took four seconds to learn about the plumber.
- Structured JSON logs; `txtodo doctor` for humans.
- A Grafana dashboard JSON lives in `deploy/grafana/`. Yes, "tasks completed per week by project" is a panel.
- SLOs, written down: convergence within 2 s on LAN and 30 s via relay for 99.9 % of ops; zero data loss, ever.

---

## 11. Verification

- **TLA+** (`specs/sync.tla`): models devices, the op log, and the reconciler; TLC checks convergence (all online devices reach the same file) and the no-loss invariant.
- Property tests, fuzzing, and differential tests against `todo.sh` (§3).
- A multi-device **sync simulator** in CI: N virtual devices, random ops, random partitions, random external edits; assert convergence and no loss after healing.
- Nix flake for reproducible builds; `cargo-zigbuild` for cross-compilation; signed releases via Sigstore; SBOM.
- CI matrix: macOS, Linux (glibc + musl), Windows, iOS simulator, Android emulator, WASM.

---

## 12. Repository layout

```
txtodo/
├── crates/
│   ├── txtodo-core/      parser · model · formatter · diff   (no_std)
│   ├── txtodo-query/     query language
│   ├── txtodo-crdt/     document model · reconciler
│   ├── txtodo-sync/     protocol · transports · pairing · crypto
│   ├── txtodo-store/    op log · snapshots · index
│   ├── txtodo-daemon/   txtodod
│   ├── txtodo-mcp/      MCP server
│   ├── txtodo-api/      gRPC + REST            (M10; not in plan §2 — plan wins)
│   ├── txtodo-plugins/  WASM host              (M10; not in plan §2 — plan wins)
│   ├── txtodo-cli/      txtodo
│   ├── txtodo-tui/
│   └── txtodo-ffi/      uniffi · wasm-bindgen · cbindgen · pyo3 · napi
├── apps/
│   ├── desktop/           Tauri
│   ├── ios/
│   ├── android/
│   └── web/
├── plugins/
├── relay/                 reference relay
├── specs/                 todotxt.abnf · sync.tla · conflicts.md
├── deploy/                grafana · systemd · launchd · nix
└── corpus/                real-world todo.txt files (anonymised) for round-trip tests
```

---

## 13. Build order (the actual MVP inside the over-engineering)

If you want to *use* it before you finish it:

1. `txtodo-core` + `txtodo` CLI with todo.sh parity. Useful on day one.
2. `txtodod` + file watcher + reconciler + op log. History and undo on one device.
3. CRDT + LAN sync + pairing. Two laptops on the same Wi-Fi.
4. MCP server, stdio first, then HTTP + tokens. Agents.
5. Relay + hole-punching. Phone to laptop across the internet.
6. Desktop and Android (Android before iOS, because it can run the daemon properly).
7. iOS, web, editors, plugins, observability, TLA+.

Steps 1–4 are a couple of months for one person. Step 7 is a lifestyle.

---

## Appendix A — extension tags (normative)

| Key | Value | Written by txtodo? | Safe to strip? |
|---|---|---|---|
| `id` | ULID | yes, in tagged mode | yes; falls back to fingerprints |
| `pri` | `A`–`Z` | on completion | yes; priority is lost, as the spec warns |
| `due` | `YYYY-MM-DD` | only if the user or an agent sets it | yes |
| `t` | `YYYY-MM-DD` | only if set | yes |
| `rec` | `[+]N[dwmy]` | only if set (plugin) | yes |
| `h` | `1` | only if set | yes |
| `ref` | directory slug | on first write into a task's notes or sub-list | yes; the directory stays, the link is lost |

## Appendix B — things deliberately *not* over-engineered

- The file format. It's todo.txt. One line, one task.
- The relay. It stores blobs it can't read and forwards pushes. That is all it will ever do.
- Task IDs in the UI. You see line numbers, like todo.sh; ULIDs are for machines.
- Accounts. There aren't any. There are devices, and a group of them that trust each other.
