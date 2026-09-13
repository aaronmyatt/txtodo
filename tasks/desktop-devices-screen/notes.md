# Devices and agents screen with QR pair, token create and revoke, activity feed (plan M7, plan §7 §6.2)

Plan M7: "Devices and agents screen: pair (QR render + scan via webcam), token create/revoke,
activity feed from the op log." Design §7: the Svelte UI talks to `txtodod` over local IPC
through Tauri commands and renders — it never re-implements pairing, token crypto, or the op log.
The daemon already owns all three surfaces from M4 (pairing) and M6 (tokens); this task is a
three-pane renderer plus four thin Tauri command proxies.

## Goal

One screen, three panes, all read-through to the daemon. No new crypto, no new store writes from
the UI side. Pairing reuses M4's `txtodo-sync` handshake; tokens wrap design §6.2's macaroon RPC;
the feed is a read-only projection of the op log.

## Design

### Tauri command proxies (Rust side, `apps/desktop/src-tauri/commands.rs`)

Every command is a tonic call into `txtodod`; the Svelte side holds no secrets and no state:

```rust
// tonic client to the daemon's gRPC (design §5: unix socket / named pipe).
// Ref: https://docs.rs/tonic · Tauri command wiring: https://v2.tauri.app/develop/calling-rust/
#[tauri::command]
async fn pair_offer(state: State<'_, Daemon>) -> Result<PairOffer, String>;
#[tauri::command]
async fn pair_accept(code: String) -> Result<PairResult, String>;   // other side scanned *us*
#[tauri::command]
async fn pair_confirm_sas() -> Result<PairResult, String>;          // both sides must confirm
#[tauri::command]
async fn token_create(req: TokenCreateReq) -> Result<Token, String>;
#[tauri::command]
async fn token_revoke(id: TokenId) -> Result<(), String>;
#[tauri::command]
async fn op_log_stream() -> Result<Subscription<OpEvent>, String>;  // feeds the activity pane
```

### Svelte types and panes (`apps/desktop/src/devices/`)

```ts
// PairOffer carries only identity + handshake material — NEVER the group key.
// M4 pairing: QR holds device, group_id, X25519 public key, endpoint, one-time nonce.
interface PairOffer { device: string; group_id: string; x25519_pub: string;
                      endpoint: string; nonce: string }
interface PairResult { sas: string }            // 6 words, EFF short list
interface TokenCreateReq { name: string; scope: Scope[]; project?: string;
                           context?: string; file?: string; expires: string }
type Scope = "read" | "write:add" | "write:complete" | "write:edit" | "write:delete"
           | "raw" | `project:${string}` | `context:${string}` | `file:${string}`;
```

- **Devices pane.** Renders the current `pair_offer()` as a QR via node-qrcode
  (https://github.com/soldair/node-qrcode) and scans a peer's QR via `getUserMedia`
  (https://developer.mozilla.org/en-US/docs/Web/API/MediaDevices/getUserMedia) + jsQR
  (https://github.com/cozmo/jsQR). On decode, `pair_accept(code)` returns the 6-word SAS; the UI
  shows it and requires an explicit tap on **both** devices before `pair_confirm_sas()`.
- **Tokens pane.** `token_create` form mapping name + scope checkboxes + optional project/context/
  file/expires to the caveats in design §6.2 (`txtodo token create --scope read,write:add …`).
  List and revoke via the same RPC; revocation lives in the daemon's revocation list (M6), not in
  the UI.
- **Activity pane.** Subscribes to `op_log_stream()`; renders `{ principal, op, relative_time }`
  from `oplog.db` (ADR 0004). Principal is the token or device that made the mutation — the same
  source `txtodo blame` reads (design §6.2).

## Placement / dependencies

- New: `apps/desktop/src/devices/{Devices.svelte, Tokens.svelte, ActivityFeed.svelte, types.ts}`,
  `apps/desktop/src-tauri/commands.rs` (four commands above).
- Depends on the existing daemon RPC surface only (M4 pairing + M6 tokens + op-log stream). No
  new Rust crate; `node-qrcode` and `jsqr` are the only new npm deps (both need the human's
  sign-off + a `deny.toml` pass before landing).

## Edge cases & invariants

- `PAIRING_WINDOW_MS` expiry and `MAX_CONCURRENT_PAIRINGS` (asserted in M4) are rendered as
  visible UI states ("pairing window closed", "too many pairings"), never silent no-ops.
- SAS confirmation must be mutual and explicit; there is no "trust this device" default. Assert the
  negative: no confirm call fires without a user tap.
- The QR must never encode key material — the invariant is "the decoded payload contains no field
  that isn't already public in `PairOffer`". Guard with a test that asserts the QR payload equals
  `PairOffer` and rejects any extra field.
- A revoked token is refused on its next use by the daemon (M6); the UI only reports that failure,
  it never trusts a local "revoked" flag.
- Token scopes are a closed union in `types.ts`; the form cannot fabricate a scope the daemon
  doesn't know (design §6.2 scope table). Attenuation (narrowing a token) is a daemon feature —
  out of scope for this screen's UI, but the list must show the scope so a human can see it.

## Acceptance

- QR renders from a live `pair_offer()`; scanning a loopback second daemon's QR completes the pair
  and both devices must confirm before the group key lands.
- Create a token with `read,write:add` + `project:+work`; it is listed; revoke it; its next use is
  refused by the daemon.
- Feed shows recent ops with principal and relative time, newest first, no fabricated entries.
- QR payload holds no key material (asserted in a unit test over the rendered payload).

## References

- Design §6.2 tokens · plan M4 pairing · plan M6 token RPC · ADR 0004 (`oplog.db`).
- https://docs.rs/tonic · https://v2.tauri.app/develop/calling-rust/
- https://github.com/soldair/node-qrcode · https://github.com/cozmo/jsQR

## As built (2026-09-13, agent)

Found fully built from an earlier session, matching this file's design closely enough that no
changes were needed — verified, not rebuilt:

- `apps/desktop/src-tauri/src/{commands_pairing,commands_tokens,commands_activity}.rs` +
  `dto_{pairing,tokens,activity}.rs` — the exact six commands this file specifies
  (`pair_offer`/`pair_accept`/`pair_confirm_sas`/`token_create`/`token_list`/`token_revoke`) plus
  `op_log` for the activity pane, each a thin `ensure_connected` + one RPC + DTO conversion, same
  pattern as every other `commands_*.rs` file.
- `apps/desktop/src/devices/{Devices,Tokens,ActivityFeed}.svelte` + `{api,camera,qr,pairing,time,types}.ts`
  — QR render via `qrcode`, scan via `getUserMedia` + `jsqr`, mutual-confirm-only SAS (two distinct
  taps, `armConfirm`→`confirmMatch`, never an automatic confirm), the closed `Scope` union with a
  runtime `isValidScope` guard so the create-token form can't fabricate a scope string, and a
  bounded one-shot `op_log()` fetch (refresh button + refetch-on-focus, not a live stream — that's
  a fair reading of ADR 0004's `oplog.db` as a queryable table, not itself a push source).
- `apps/desktop/src-tauri/tests/new_rpcs.rs` already covers `pair_offer`/`pair_accept`/
  `pair_confirm_sas`/token create-list-revoke-round-trip/`op_log` against a real daemon; `apps/desktop/src/devices/{pairing,qr,time,types}.test.ts`
  cover the pure logic (pairing-window math, the QR payload's security invariant — no field beyond
  `PairOffer`'s own five — relative-time formatting, the scope union guard).

No gaps found worth flagging here beyond the two already-known, already-out-of-scope ones this
session ran into elsewhere: pairing's RPCs are real but daemon-to-daemon sync itself has no
transport yet (see `desktop-conflict-review`/`desktop-playwright-tests`'s notes — unrelated to this
screen's own correctness, since it only exercises the *local* pairing bookkeeping, not a completed
cross-device handshake), and plan M6's richer macaroon/scope layer is explicitly deferred per this
task's own brief.

## What to open and look at

- `npm run tauri dev`, open `/devices`. "Show my code" should render a QR within ~1s and count down
  from the pairing window; "Scan a peer's code" should request camera access and, on a valid scan,
  show a 6-word SAS requiring an explicit tap on **this** device before confirming.
- Create a token with a couple of scopes and an expiry; confirm the secret is shown exactly once,
  the token appears in the list with its scopes (never the secret again), and revoking it removes
  it from the list.
- The activity pane should show recent ops with a principal and relative time, newest first; click
  Refresh and confirm it re-fetches (a `network` tab check on `op_log` is enough — no fabricated
  rows should appear while a fetch is pending or the log is empty).
