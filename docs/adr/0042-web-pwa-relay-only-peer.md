# 0042 — Build the web PWA as a relay-only iroh peer running the daemon's engine in wasm

- Status: accepted 2026-10-17 (decided by the agent at the owner's request; plan in
  `tasks/web-pwa/notes.md`). The two spikes at the top of `tasks/web-pwa/todo.txt` can reopen
  point 3: if iroh cannot run in a Worker, a superseding ADR picks the carrier again.
- Date: 2026-10-17
- Deciders: agent, at the owner's request

## Context
With native mobile (M9) parked, a browser app is the phone client, and the line asks for offline
use with browser storage. Three facts, checked on 2026-10-17:

- iroh 1.2 builds for `wasm32-unknown-unknown` (`wasm_browser` cfg). It runs relay-only, with QUIC
  inside the relay's WebSocket.
- The merge engine (`DocState`, apply, reconcile, text history) is about 4.6k LOC of pure code
  inside `txtodo-daemon`, with no store trait.
- The desktop Svelte frontend already runs in a plain browser behind one shim (`tauriShim.ts`).

## Decision
1. **A full peer.** The PWA is a sync peer: its own device identity, its own op log and offline
   edits. It joins the device set like any device (ADR 0021). A dedicated Web Worker is its daemon,
   and the page is a thin client of that Worker, so design §7's thin-client rule holds between the
   page and the Worker.
2. **`txtodo-engine`.** A new crate takes the daemon's pure engine modules and the pure half of the
   actor glue, and the daemon depends on it. Behaviour does not change; the tests move with the
   code.
3. **Carrier.** An iroh `wasm_browser` endpoint, relay-only, on ALPN `txtodo/sync/1` and protocol 5,
   with messages sealed as today. No new wire format, no daemon change. The `relay/` mailbox is
   not used.
4. **Store.** A trait over the op-log methods the file path uses:
   - SQLite stays for the daemon.
   - In the browser, an OPFS append-only op log plus a snapshot, written through
     `createSyncAccessHandle` in the Worker.
   - An in-memory version for tests.
5. **`txtodo-sync`.** `WorkspaceId` moves to `txtodo-model`. A default `native` feature gates
   iroh's tokio side, mdns-sd, keyring and the fs carriers.
6. **`txtodo-web`.** A new wasm-bindgen crate hosts the engine, store, keys and endpoint in the
   Worker, and answers the desktop frontend's command set.
7. **UI.** `apps/desktop` gains `--mode web`, which aliases `@tauri-apps/api/*` to a Worker shim,
   as `--mode e2e` does. `specs/client-parity.toml` rows gain an optional `web` field, for `na`
   rows only; a missing field means "as desktop". This extends ADR 0031.
8. **Keys.** Stored in IndexedDB as raw bytes wrapped by a non-extractable WebCrypto AES-GCM key,
   behind `txtodo-sync`'s `KeyStore` trait.
9. **Tabs.** One tab owns the data, through a Web Lock. Other tabs say so and stay read-only.
10. **Hosting.** Cloudflare Workers static assets, on its own `workers.dev` origin, with a
    `_headers` file setting a CSP and `frame-ancestors 'none'`. Not GitHub Pages.

## Options
- **Thin client to a daemon over iroh.** No Rust moves. Rejected: no offline use, and a daemon must
  be online for every read.
- **Our own WebSocket protocol to the `relay/` mailbox.** Its store-and-forward would close the
  "another device must be online" gap. Rejected for now: a new wire format, and the mailbox has no
  auth and no deploy (ADR 0027). Revisit as an additive carrier.
- **SQLite compiled to wasm with an OPFS VFS.** Rejected: `libsqlite3-sys` 0.38 bundles a WASI
  build only, so this needs a C toolchain in the wasm build and a second SQLite.
- **A new `apps/web` UI.** Rejected: a third client to keep at parity.
- **GitHub Pages.** The project site would be `https://aaronmyatt.github.io/txtodo/`, one origin
  with the owner's 21 other Pages sites. OPFS and IndexedDB are per origin, so script on any of
  those sites could read the lists and unwrap the group key, and Pages cannot set response headers.
  A Pages site under a new org gets its own origin but still no headers. Rejected.
- **A leader tab with BroadcastChannel followers.** Later, if one owning tab turns out to hurt.

## Consequences
- Good:
  - An offline phone client.
  - One engine for the daemon and the browser.
  - No wire or relay change.
  - The desktop UI and its Playwright specs are reused.
- Bad:
  - Two new crates, a store trait and a sync feature split: a big daemon refactor before any
    browser code exists.
  - The browser syncs only while another device is online, because the relay forwards and does
    not store.
  - Eviction loses ops that were never synced: storage pressure, or iOS Safari's 7-day cap for
    sites not added to the home screen.
  - Script on the same origin can unwrap the keys; the CSP is the guard.
  - Cloudflare is a new vendor and a new account.
- Neutral / follow-ups:
  - The frozen paths (root `Cargo.toml`, `.github/**`) and `.claude/budgets.json` `allowedDeps`
    change for the two crates.
  - Moving the origin later makes every browser pair again, so settle the final origin (a custom
    domain, if one is wanted) before anyone relies on it.
  - Op signatures still use the group-derived key (`lan_op_signing.rs`); that does not change here.

## References
- iroh 1.2: https://docs.rs/iroh/1.2.0/iroh/
- OPFS: https://developer.mozilla.org/en-US/docs/Web/API/File_System_API/Origin_private_file_system
- Same-origin policy (storage is per origin):
  https://developer.mozilla.org/en-US/docs/Web/Security/Same-origin_policy
- `wrapKey`: https://developer.mozilla.org/en-US/docs/Web/API/SubtleCrypto/wrapKey
- Web Locks: https://developer.mozilla.org/en-US/docs/Web/API/Web_Locks_API
- Workers static assets headers: https://developers.cloudflare.com/workers/static-assets/headers/
- Pages to Workers: https://developers.cloudflare.com/workers/static-assets/migration-guides/migrate-from-pages/
- CSP `frame-ancestors`:
  https://developer.mozilla.org/en-US/docs/Web/HTTP/Reference/Headers/Content-Security-Policy/frame-ancestors
