# apps/desktop/src: instrument tauriShim.ts invoke/listen (root todo.txt line 36)

## Goal

Root todo line (`ref:logging-frontend`) asks for: wrap `tauriShim.ts`'s `invoke` and `listen` —
the single chokepoint all 25 frontend->Rust commands and both event streams (`daemon-status`,
`daemon-change`) pass through — emitting `ui_invoke_start`/`ui_invoke_ok`/`ui_invoke_err` (fields:
`command`, `ms`, `request_id`) and `ui_event` (for event arrivals), forwarded through the real
`ui_log` Tauri command into the same Rust log timeline `logging-desktop` built. Frontend
counterpart to that task; its "As built" explicitly scoped `.ts`/`.svelte` files out.

## Local request_id (not a shared id)

Root line's own "known limitation" paragraph: correlation to the Rust-side span stays
timestamp+command-name based — threading one real id through all 24 (now 26) command signatures
would be a breaking change, out of scope here. But the line still asks for a `request_id` field on
the emitted events, so: mint a short local, frontend-only, incrementing id
(`` `r${++counter}` ``, monotonic per page load) at the start of each `invoke()` call, purely to
let someone reading the JSON log line up that one call's own start/ok/err triplet. It carries no
meaning on the Rust side and is never sent as a real argument to any Tauri command — it only rides
along inside the `ui_log` `fields` payload.

## Design

`tauriShim.ts` keeps `invoke`/`listen`'s exact existing signatures and behavior (return values,
error propagation, sync-vs-async shape) — this only wraps them, it doesn't change what callers see.

- A private `logToRust(level, message, fields)` helper calls `tauriInvoke<void>("ui_log", { level,
  message, fields })` (the *real* `@tauri-apps/api/core` invoke, not this module's own `invoke` —
  logging itself must never recurse through the wrapper it's part of, and must never go through
  `mockInvoke`, which has no `"ui_log"` case and would throw `unhandled command`). Wrapped in
  `.catch(() => {})`: a logging failure must never surface to, or throw for, the caller's own
  business logic — `ui_log` returning `Err` (or the call rejecting outright, e.g. no Tauri runtime)
  is swallowed silently, matching the brief's "ui_log returning Err should never itself throw and
  break the caller's own instrumentation logic".
- `invoke()`: mints `request_id`, records `performance.now()`, calls through to the existing
  `hasTauri() ? tauriInvoke : mockInvoke` dispatch unchanged, then on settle computes `ms =
  performance.now() - start` and fires exactly one of:
  - `ui_invoke_ok` (`level: "info"`) with `{ command, ms, request_id }` on resolve.
  - `ui_invoke_err` (`level: "warn"`) with `{ command, ms, request_id, error }` on reject —
    `error` is `String(err)` (an `Error.message`/thrown-value stringification), never the
    original `args` — then re-throws the original error unchanged so callers see identical
    behavior to today.
  - `ui_invoke_start` (`level: "info"`) fires synchronously before dispatch, with `{ command,
    request_id }` (no `ms` yet — nothing has elapsed).
  - Level choice: `start`/`ok` are routine traffic (`info`); `err` is `warn` not `error` — an
    `invoke` rejection is often an expected/handled condition in this app (e.g. `list_conflicts`
    on an unpaired workspace, a cancelled dialog) rather than a crash; `commands.rs`'s `ui_log`
    itself only distinguishes exactly the 5 tracing levels, so `warn` here is a judgment call
    documented rather than a hard requirement.
- `listen()`: unchanged dispatch (`hasTauri() ? tauriListen : mockListen`), but the `handler`
  passed through is wrapped so every payload arrival fires `ui_event` (`level: "info"`) with
  `{ event, request_id }` (a fresh id minted per *arrival*, not per subscription — each event
  delivery is its own loggable occurrence) before calling the caller's real handler unchanged
  (same argument, same return value/await behavior). The `listen()` call itself does **not** emit
  `ui_invoke_start/ok/err` — those are `invoke`-specific per the root line's wording ("emitting
  ui_invoke_* ... and ui_event for daemon-status/daemon-change arrivals"); subscribing is not a
  loggable "call succeeded/failed" event the way a one-shot `invoke` is.

## Placement

All in `apps/desktop/src/lib/tauriShim.ts` — no new files for the implementation. New test file
`apps/desktop/src/lib/tauriShim.test.ts` (colocated, matching `lib/stores/conflicts.test.ts`'s
convention — this crate has no `slices.root` fence, so no lease concern, but tests still live next
to the file they test per existing repo convention rather than under `__tests__/`, since `stores/`
does the same and `__tests__/` here is only used by two components/one todotxt module).

## Edge cases

- **No content leakage**: `fields` on every emitted event is built from a closed, explicit object
  literal (`{ command, ms, request_id }` etc.) — never spreads or forwards the caller's `args`
  (which may carry real task/note text, e.g. `apply`'s `mutations`, `edit_notes`'s `newText`).
  `ui_invoke_err`'s `error` field is the stringified error/exception, never the original request
  args either.
- **Mock backend must keep working**: `logToRust` always calls the *real* `@tauri-apps/api/core`
  `invoke`, bypassing `hasTauri()`/`mockInvoke` entirely. Under `npm run dev` (no Tauri runtime)
  that call rejects (no `__TAURI_INTERNALS__`, no IPC channel) — caught and swallowed by the
  `.catch(() => {})` above, so `mockInvoke`/`mockListen`'s actual command dispatch and every
  existing mock-backed test is untouched and unaffected.
- **`listen()`'s wrapped handler must not change control flow**: if the caller's handler is async
  and its promise matters to `@tauri-apps/api/event`'s typings (`EventCallback<T>` returns `void`
  in the real API, so nothing awaits it today) — the wrapper preserves this: fire-and-forget
  `logToRust`, then call the real handler with the same `event` object, no `await` inserted that
  wasn't already absent.
- **Timing source**: `performance.now()` (available in every browser + Tauri's webview, monotonic,
  sub-ms) not `Date.now()` — matches the "elapsed ms" the Rust side's own spans measure via
  `tracing`'s span timing, and avoids wall-clock jumps skewing `ms`.

## Acceptance

- `tauriShim.ts`'s exported `invoke`/`listen` signatures and runtime behavior (return values,
  thrown errors, mock-vs-real dispatch) are byte-identical to before this change from every
  existing caller's perspective.
- A new `tauriShim.test.ts` asserts, using the mock backend (`hasTauri()` false in a `jsdom`/node
  vitest environment — no `window.__TAURI_INTERNALS__`): `invoke()` still resolves with
  `mockInvoke`'s real return value; a `ui_log`-shaped call (`tauriInvoke("ui_log", {...})`) is
  attempted for start+ok (spy on `@tauri-apps/api/core`'s `invoke`, since that's what `logToRust`
  calls, not this module's own wrapped `invoke`) with the documented field shape; a rejected
  `mockInvoke` call still rejects with the original error while also firing `ui_invoke_err`; no
  emitted `fields` object ever contains a raw task/note string passed as an arg.
- `npm run check`, `npm run test` (vitest) green. `npm run lint` only if `package.json` actually
  defines one (see "As built" below for what existed).

## As built (2026-09-16, agent)

Built exactly to the design above; no structural deviations. `apps/desktop/package.json` has no
`lint` script (only `check` and `test`), confirmed before assuming — ran the two that exist.

- `apps/desktop/src/lib/tauriShim.ts`: added `nextRequestId()` (module-level incrementing counter,
  `r1`, `r2`, ...), `logToRust(level, message, fields)` (always calls the real
  `@tauri-apps/api/core` `invoke("ui_log", ...)`, `.catch(() => {})`'d), and wrapped `invoke`/
  `listen` around it. Both functions' exported signatures are byte-identical to before.
- `apps/desktop/src/lib/tauriShim.test.ts` (new): 6 tests, mocking only `@tauri-apps/api/core`'s
  `invoke` export (spied via `vi.mock`) — `@tauri-apps/api/event` needed no mock since `hasTauri()`
  is false in plain-Node vitest (no `window`, `import.meta.env.MODE !== "e2e"`), so `listen()`
  dispatches through the real `mockListen`/`./mock/state.ts` event bus unchanged, same as
  `npm run dev`.

### Verification

- `npm run test` (vitest): 9 files, 102/102 passed, including the 6 new ones.
- `npm run check` (svelte-check): 374 files, 0 errors, 0 warnings.
- Mock backend confirmed unaffected: `invoke("daemon_status")` still resolves `"connected"` (the
  literal value `mockInvoke`'s own `daemon_status` case returns), and a rejected `mockInvoke` call
  (`get_file` on an unknown path) still rejects the caller with its original, unmodified error —
  proven by a passing `rejects.toThrow(/unknown path/)` assertion — while *also* firing
  `ui_invoke_err`. `listen("daemon-status", ...)` still receives the mock event bus's real payload
  (`emit("daemon-status", "connected")` from `./mock/state.ts` reaches the caller's handler
  unchanged) while *also* firing `ui_event`.
- No argument-content leakage: one test calls `invoke("apply", { path: "todo.txt", mutations: [{
  kind: "add", line: <real task text> }] })` and asserts none of the `ui_log`-bound payloads'
  serialized JSON contains that text, and that no `fields` object ever carries a `mutations` or
  `args` key. `ui_invoke_err`'s fields are asserted to be the closed set `{command, error, ms,
  request_id}` — never a spread of the call's original `args`.
- One deviation from the notes' own "Acceptance" wording, deliberate: dropped an initial assertion
  that a `get_file` 404 error's `error: String(err)` text should not contain the requested path —
  that path string appears only because `mockInvoke`'s own `Error` message includes it (`` `mock
  daemon: unknown path "${path}"` ``), same as the Rust side's real `DaemonError::Display` text
  legitimately carrying transport detail (`logging-desktop`'s own notes, "Verification" section).
  This is not the wrapper spreading raw `args` (the leakage this task's brief actually warns
  about, contrasted there with task/note text) — a file path riding inside an error's own message
  is not `fields` "carrying real task/note text" the way an `apply` mutation's `line` would.
  Replaced with the `Object.keys(errFields)` closed-set assertion above, which is the property the
  design section actually specifies.

### Deliberately out of scope

- Any Rust file — `ui_log` already exists and is done (`logging-desktop`).
- Any other `+m11 @observability` backlog line, or any file outside `apps/desktop/src/`,
  `tasks/logging-frontend/`, and this one root todo.txt line.
- `QuickAdd.svelte`'s own direct `listen("quick-add-shown", ...)` import from
  `@tauri-apps/api/event` (not through `tauriShim.ts`) — untouched; the root line names
  `daemon-status`/`daemon-change` specifically, both of which go through `daemon.ts`, which already
  imports `listen` from this shim.
