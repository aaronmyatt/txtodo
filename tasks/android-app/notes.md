# Android foreground-service daemon, Compose UI with AnnotatedString from tokenize, LAN MCP, FCM (plan M9, plan §7)

## Goal

`apps/android/` is a Kotlin/Jetpack Compose app whose UI is the file, per plan §3.1–3.2. It is
thin: it never parses a line itself. It renders `todo.txt` with `AnnotatedString` built from
`txtodo_core::tokenize` spans (via uniffi) so token boundaries are byte-identical to every other
platform. A foreground service (type `dataSync`) hosts the `DaemonHandle` in-process so the daemon
survives activity death; the UI binds to the service. MCP is served over HTTP on LAN only while the
service runs; FCM registers the device so the M8 relay can forward wake-ups.

## Design

The FFI boundary is [ffi-uniffi-daemonhandle](../ffi-uniffi-daemonhandle/notes.md): `tokenize(raw)
-> Vec<Span>` with `Span { kind, start, end }` and `DaemonHandle { new(root), apply, get, subscribe }`.
`kind` is one of the nine §3.1 semantic names — `priority`, `date`, `completion-marker`, `project`,
`context`, `tag-key`, `tag-value`, `id-tag`, `text` — mapped to `SpanStyle` in one place.

```kotlin
// apps/android/app/src/main/kotlin/dev/txtodo/app/DaemonService.kt
// type dataSync keeps the daemon alive across activity death.
// Ref: https://developer.android.com/develop/background-work/services/fgs
class DaemonService : Service() {
    private lateinit var handle: DaemonHandle            // uniffi-generated, holds a tokio runtime
    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        startForeground(NOTIF_ID, buildNotification(), FOREGROUND_SERVICE_TYPE_DATA_SYNC)
        scope.launch { handle = DaemonHandle.new(workspaceRoot()) }   // embeds txtodod in-process
        return START_STICKY
    }
    override fun onBind(intent: Intent?): IBinder = binder   // LocalBinder exposing the handle
}

// apps/android/app/src/main/kotlin/dev/txtodo/app/FileViewModel.kt
// id: tags are filtered from the attributed string by default (plan §3.1); offsets must not drift.
fun spansFor(raw: String): AnnotatedString {
    val spans = tokenize(raw)                            // full raw line, spans cover every byte
    val drop = spans.filter { it.kind == SpanKind.ID_TAG }  // remove only the id: span + its space
    val display = raw.removeRange(drop)                  // one contiguous removal; offsets recomputed below
    val b = AnnotatedString.Builder(display)
    for (s in retokenizedDisplaySpans(display)) b.addStyle(styleFor(s.kind), s.start, s.end)
    return b.toAnnotatedString()
}
```

- Main view: `LazyColumn` of `AnnotatedString` rows with real line numbers, blank lines shown and
  numbered; completed lines muted + description struck through; `ref:` lines show a trailing `n/m`
  indicator (notes-only → notes icon); long-press reveals the pencil; last row is always an empty
  "Add a line" row. Header toggle shows `id:`. `AnnotatedString`:
  https://developer.android.com/reference/kotlin/androidx/compose/ui/text/AnnotatedString
- Single tap → edit popover: single-line field pre-filled with the *raw* line (including `id:`),
  token chips `(A)(B)(C) + @ due: t: rec: x` per §3.2; `x` toggles completion (`x <today> ` +
  `pri:` preservation); strict-mode validation via the FFI parser, lenient save still allowed
  (never block saving text); footer `Line N · <device>, <relative time>` from the op log.
- Double tap → detail view: breadcrumb `todo.txt › 2 › q4-roadmap/todo.txt › 3`, pinned parent,
  `notes.md` editor, recursive sub-list (`n of m done` header); both sections render even when
  files don't exist yet; first keystroke creates the directory (ref rule 4).
- LAN MCP: reuse `txtodo-mcp` in-process; `--lan` binds `0.0.0.0` and advertises
  `_txtodo-mcp._tcp` on `8636` (plan §1 decision 10). Non-loopback is refused unless `--lan`
  (security checklist). The service keeps the MCP listener alive only while foreground.
- FCM: register the device token with the daemon via `FirebaseMessaging.getInstance().token`; on
  `onNewToken` re-register. The M8 relay forwards wake-ups to this token when a peer posts ops.
  https://firebase.google.com/docs/cloud-messaging/android/client

## Placement/dependencies

- New directory `apps/android/` (Gradle, Kotlin, Compose) — not a Cargo workspace member; root
  `Cargo.toml` is untouched. `apps/android/` is not a frozen path.
- Depends on `ffi-uniffi-daemonhandle` (uniffi bindings + `DaemonHandle`), `txtodo-mcp` (LAN MCP
  host), and M8 `relay/` (FCM wake-up forwarding). No new Rust crates, no new frozen-path writes.
- Gradle + Kotlin lint/typecheck/test commands must be added to the mobile second-stack mapping
  (same mechanism as `desktop-stack-mapping`), not to the Rust `justfile`.

## Edge cases & invariants

- OS kills the service → `START_STICKY` + `startForeground` recreates it; the daemon state is
  rebuilt from the files (files are the truth — nothing lives only in memory).
- Process death → `DaemonHandle::drop` shuts the runtime down cleanly (no leaked sockets/locks).
- `id:` filtering: the removed span is exactly one `id-tag` span plus its separating space;
  recomputing offsets after removal keeps byte-identical boundaries (assert: re-tokenizing the
  display string yields the same non-`id:` span sequence).
- Invariant (assert the negative): no Kotlin module imports `File`/`FileReader` for parsing; every
  mutation is a `DaemonHandle.apply` call. The only file I/O is inside the daemon.
- MCP `--lan` is the only path that binds a non-loopback address; a foreground-service restart must
  re-apply it, not default to `127.0.0.1`.

## Acceptance

- Instrumented test on device/emulator: `tokenize` → `AnnotatedString` spans match the expected
  `SpanKind` per line, `id:` absent by default, completed line struck through.
- LAN MCP: an MCP client on the same network calls a tool and gets a result while the service runs.
- Activity recreation (rotate / background) leaves the daemon alive and the list re-binds without a
  re-parse.
- FCM token is registered with the daemon and re-registered on `onNewToken`.

## References

- plan M9 + §3.1–3.2 (txtodo-implementation-plan.md); design §7 Android row (txtodo-design.md)
- uniffi Kotlin async: https://mozilla.github.io/uniffi-rs/latest/kotlin/async.html
- [../ffi-uniffi-daemonhandle/notes.md](../ffi-uniffi-daemonhandle/notes.md)
