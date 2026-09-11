# iOS SwiftUI over UITextView, embedded daemon, BGAppRefresh, silent push, App Intents, share sheet (plan M9, plan §7)

## Goal

`apps/ios/` is a SwiftUI app whose UI is the file (plan §3.1–3.2). The daemon runs embedded in the
app process via the `DaemonHandle` from [ffi-uniffi-daemonhandle](../ffi-uniffi-daemonhandle/notes.md);
there is no separate daemon process on iOS. Editable text renders through a `UITextView` with
`NSTextStorage`/`NSAttributedString` highlighting from `tokenize` (plan §1 decision 8), so
highlighting survives typing. Background work is `BGAppRefresh` + APNs silent push; system
integration is App Intents and a share sheet.

## Design

Same FFI boundary as Android: `tokenize(raw) -> [Span]` with `Span { kind, start, end }`, the nine
§3.1 semantic names, and `DaemonHandle { new(root), apply, get, subscribe }` (uniffi async → Swift
`async`). The span→`NSAttributedString` mapping is unit-tested byte-identical to the Android mapping
(one generated enum, two consumers).

```swift
// apps/ios/Txtodo/AttributedLine.swift
// id: filtered from the attributed string by default (plan §3.1); the removal is one id-tag span.
func attributedLine(_ raw: String) -> NSAttributedString {
    let spans = tokenize(raw: raw)                       // [Span], uniffi-generated
    let idSpans = spans.filter { $0.kind == .idTag }     // drop exactly the id: span + its space
    let display = removingSpans(raw, idSpans)
    let out = NSMutableAttributedString(string: display)
    for s in retokenize(display) {                       // recomputed offsets, same non-id spans
        out.addAttribute(.foregroundColor, value: color(s.kind),
                         range: NSRange(location: s.start, length: s.end - s.start))
    }
    if completed(display) { /* .strikethroughStyle on the description range only */ }
    return out
}

// apps/ios/Txtodo/EditorTextView.swift
// UITextView preserves attributed spans while editing — the reason we do not use TextField.
// Ref: https://developer.apple.com/documentation/uikit/uitextview
struct EditorTextView: UIViewRepresentable {
    func makeUIView(context: Context) -> UITextView { /* NSTextStorage-backed, editable */ }
    func updateUIView(_ uiView: UITextView, context: Context) { /* re-apply spans on Watch changes */ }
}
```

- Main view: SwiftUI list of `AttributedString` rows with real line numbers (blank lines numbered);
  completed lines muted + struck through; `id:` hidden with a header toggle; `n/m` ref indicator /
  notes icon; long-press pencil; trailing "Add a line" row.
- Single tap → edit popover (raw line + chips `(A)(B)(C) + @ due: t: rec: x`, `x` toggle preserves
  `pri:` per spec), strict validation via FFI, lenient save allowed. Double tap → detail view
  (breadcrumb, pinned parent, notes editor, recursive sub-list, `n of m done`); first keystroke into
  empty notes creates the directory (ref rule 4).
- `BGAppRefresh`: schedule a `BGAppRefreshTask` that drives a daemon reconcile then refreshes
  widgets; the OS throttles this, so also reconcile on foreground.
  https://developer.apple.com/documentation/backgroundtasks/bgapprefreshtask
- Silent push: APNs `content-available: 1` notification wakes the app to reconcile; the M8 relay
  forwards APNs to the registered device token. Treated as a hint, never the only sync path.
- App Intents `AddTodo` / `CompleteTodo` / `ListTodos` over the `DaemonHandle`, runnable from
  Shortcuts: https://developer.apple.com/documentation/appintents
- Share sheet: accept `UTType.text` and add it as a todo through the daemon (no re-parse in the
  extension).

## Placement/dependencies

- New directory `apps/ios/` (Xcode project, Swift, SwiftUI) — not a Cargo workspace member; root
  `Cargo.toml` untouched; `apps/ios/` is not frozen.
- Depends on `ffi-uniffi-daemonhandle` (uniffi Swift bindings + `DaemonHandle`), M8 relay (APNs
  forwarding), and the mobile second-stack test mapping. No new Rust crates, no frozen-path writes.

## Edge cases & invariants

- `UITextView` must keep the attributed spans live during typing (edit via `textStorage`, not by
  replacing the string), or highlighting is lost on every keystroke.
- `BGAppRefresh` and silent push are best-effort (OS deprioritises them); the app must never depend
  on them for data safety — the files are the truth and reconcile also runs on foreground/launch.
- `id:` filtering: remove exactly one `id-tag` span plus its space, then recompute offsets so the
  surviving spans stay byte-identical to the raw line's (assert in the unit test).
- Invariant (assert the negative): no Swift code calls a parser on file text; every mutation goes
  through `DaemonHandle.apply`. No bare filename paths cross the FFI boundary — workspace-relative
  refs only.

## Acceptance

- Unit test: `tokenize` → `NSAttributedString` spans are byte-identical to the Android span mapping
  for a fixture of corpus lines (including `id:`, completed, `ref:` lines).
- App Intents `AddTodo`/`CompleteTodo`/`ListTodos` run from the Shortcuts app against the embedded
  daemon and mutate exactly one line.
- `BGAppRefresh` handler runs a reconcile and exits; a silent push triggers the same reconcile path.
- Share sheet accepts text and the daemon stamps `created:` + `id:`.

## References

- plan M9 + §1 decision 8 + §3.1–3.2 (txtodo-implementation-plan.md); design §7 iOS row
- uniffi Swift async: https://mozilla.github.io/uniffi-rs/latest/swift/async.html
- [../ffi-uniffi-daemonhandle/notes.md](../ffi-uniffi-daemonhandle/notes.md)
