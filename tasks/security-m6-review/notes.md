# Security checklist review before M6 close + re-justify payloadKB (plan M6, plan §6)

## Goal

Re-run the security checklist with the M6 surface live, and replace `budgets.json.payloadKB: null`
with a concrete number now that a real HTTP surface exists. The checklist is one line of prose in
the plan ("M4/M6/M8 gate"), enumerated in [security-m4-review](../security-m4-review/notes.md):

> no secrets in logs; keys only in keystore; every network message versioned, authenticated,
> encrypted; MCP HTTP refuses non-loopback unless `--lan`; tokens never logged; path traversal
> impossible via `ref:` (fuzz the slug validator); relay cannot distinguish op types.

## Design

### Two M4-deferred items are now buildable — they become tests, not prose

| item | M4 status | M6 check |
|---|---|---|
| MCP HTTP refuses non-loopback unless `--lan` | deferred (no HTTP surface) | test: default bind is `127.0.0.1`; a non-loopback bind errors unless `--lan` is passed (design §6.1) |
| tokens never logged | deferred (no tokens) | test: capture `tracing` across `token create`, auth, and a tool call; assert no token text, root-key bytes, or caveat values in the JSON logs |

### Re-check the inherited items — don't assume M4's results still hold

- **no secrets in logs** — re-run the M4 tracing capture against the MCP paths too (the HTTP
  middleware and stdio wiring are new logging surface).
- **keys only in keystore** — the macaroon root key is new key material: assert it exists only via
  `KeyStore::get(KeyId::TokenRoot)` and is never written outside the store (mcp-tokens).
- **every network message versioned, authenticated, encrypted** — stdio + Streamable HTTP add a
  *second* message surface alongside gRPC. Check the new surface's auth/transport properties: HTTP is
  bearer-authed (mcp-auth) but is it TLS? It is loopback by default; the `--lan` case must state its
  threat model (LAN plaintext vs. the relay's encryption in M8). Record the answer, don't assume.
- **path traversal via `ref:`** — M5 built real directory paths from slugs. Re-run the
  `fuzz/` slug target for a bounded session and record the result (the validator itself shipped M1,
  the fuzz harness in core-fuzz-targets).

### payloadKB re-justification — derive, don't pick

`budgets.json` note: "N/A until M6 … Re-justify when txtodo-mcp lands." The M0–M5 rationale was "no
HTTP surface"; that surface now exists (Streamable HTTP on `8636`). The budget governs one MCP/HTTP
response. Dominant response shapes, with the concrete caps that already exist in the code:

| response | existing cap |
|---|---|
| `todo_list` rows | `limit` arg (design §6.3); each row = raw line + parsed fields + `id`/`line` |
| `history` ops | `HistoryRequest.limit`; each op ≈ `SUMMARY_MAX_CHARS` (60, `convert.rs`) + ~100 B fixed |
| file resource `todotxt://todo.txt` / `GetFile` | `MAX_LINES_PER_FILE = 1_000_000` (`state.rs`) × line bytes — the only *unbounded* shape, since the ABNF does not bound line length |
| `todo_raw` read | `lines[]` (mcp-server-tools) |

Two gaps to close before a number is honest:

1. **No line-length cap exists.** Pin `MAX_LINE_BYTES` (new constant, same precedent as
   `FILE_PATH_MAX_BYTES = 1024` in `ids.rs`), justified by the todo.txt grammar's practical line
   size — so every per-line term in the table is bounded.
2. **The whole-file resource is unbounded.** A single response must not return `MAX_LINES_PER_FILE`
   lines at once: the file resource/`GetFile` is truncated to the payload budget (the daemon already
   truncates summaries to `SUMMARY_MAX_CHARS`; file bytes are the same discipline at a larger cap),
   or paged.

```rust
// crates/txtodo-core/src/… — the pinned cap the budget derives from
pub const MAX_LINE_BYTES: usize = 4096;           // same "pinned, named, unit-suffixed" rule as FILE_PATH_MAX_BYTES
```

Derivation (the budget is the max of the three shapes, rounded up to a power-of-two KiB):

```text
payloadKB = max( list: limit_max × MAX_LINE_BYTES,
                 history: limit_max × (SUMMARY_MAX_CHARS + 100),
                 file: truncated-to-budget )
          = max( 100 × 4096, 100 × 160, 512×1024 ) = 512 KiB
```

Recommend `payloadKB: 512`. `budgets.json` is a frozen path — land it via `/setup`, which shows the
diff before writing. The 512 KiB number is *derived from the caps above*, not picked; if the human
wants a different `limit_max` or `MAX_LINE_BYTES`, the number follows the formula, not the other way
round.

## Placement/dependencies

- `RATCHET.md` (append-only, frozen): a dated M6 section listing each checklist item as
  pass / fail / deferred-to-M8, each with the test that proves it or the reason it cannot be tested
  yet. Deferred items get a todo.txt line in the owning milestone.
- `budgets.json` (frozen): `payloadKB` change via `/setup` only. `stack.md` §"Not mechanically
  enforced" also names the payload budget and must be updated by `/setup` in the same pass.

## Edge cases & invariants

- The relay op-type-leak item stays deferred to M8 with a todo.txt line there, exactly as M4 did.
- `--lan` is the one case where MCP HTTP leaves loopback: the review must state its threat model
  (bearer auth over LAN plaintext) rather than implying encryption that doesn't exist yet.
- The slug fuzz re-run is *bounded* — a fixed duration/shrink budget, not "until clean", so the
  review itself cannot burn unbounded compute.
- Findings that are real bugs go in `RATCHET.md` (it owns exceptions and debt), never a silent fix
  inside this review task.

## Acceptance

- A dated M6 section in `RATCHET.md` lists all seven items as pass / fail / deferred-to-M8, each
  with its test (or a reason it cannot be tested).
- The two M4-deferred items now have passing tests: loopback-by-default and tokens-never-logged.
- `payloadKB` in `budgets.json` is a concrete number derived from `MAX_LINE_BYTES` × `limit` and the
  `history` row size, written by `/setup`, with `stack.md` updated in the same pass.
- The relay op-type-leak item has a todo.txt line in the M8 milestone, not a prose note.

## References

- plan M6 (txtodo-implementation-plan.md), design §6.1/§6.5 (txtodo-design.md)
- prior checklist: [../security-m4-review/notes.md](../security-m4-review/notes.md)
- caps: `MAX_LINES_PER_FILE` (`crates/txtodo-daemon/src/state.rs`), `SUMMARY_MAX_CHARS`
  (`crates/txtodo-daemon/src/convert.rs`), `FILE_PATH_MAX_BYTES` (`crates/txtodo-model/src/ids.rs`)
- frozen-path write: `/setup` skill; `budgets.json` + `stack.md` are both frozen

## As built (2026-09-24)

- Signed off by the human 2026-09-24; the entry is in `RATCHET.md` (2026-09-24, two entries: the
  loopback threat model, then the checklist). `findings-draft.md` is the signed text.
- The plan above went stale before it ran: no bearer auth, no `--lan`, no macaroon root key was
  ever built (`mcp-local-only`, ADR 0028), and `payloadKB` shipped on its own (`payload-budget`).
- Found and fixed one real bug: peer-sent `FilePath` skipped validation on decode (15ac56f).
- Added the two tests M4 deferred: loopback bind (bdbc331), token round log capture (b5d0889).
- Known gaps: tokens are never checked; other OS users reach loopback MCP; `slug_windows_safe` not
  fuzzed; no daemon-level bad-path frame test.
