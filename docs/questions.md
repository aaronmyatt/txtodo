# Open questions

Agents append here when the plan says "stop and ask" (plan §0). Humans answer under the question.
An answered question stays; its Status flips and, if it changed a decision, the ADR is linked.
Append only. Never edit a prior answer; add a dated follow-up.

## Q1 — Do non-managed files inside a `ref:` directory ever sync (attachments)?
- Status: answered 2026-09-13 · Raised: 2026-09-11 (plan §6.1) · Blocks: M5 sync scope, M8 relay payloads
- Default until answered: **no**. Only `todo.txt`, `done.txt`, `notes.md` are synced (plan §3.2.11).
- Answer: Let's limit it to only images, assuming they might be used in the markdown tasks write ups

## Q2 — Sidecar mode: confidence threshold and cost weights for fingerprint re-identification?
- Status: answered 2026-09-13 · Raised: 2026-09-11 (plan §6.2) · Blocks: sidecar identity mode,
  now pulled forward from M10 to be the default (decision reversed, see below)
- Default until answered: none needed before M10. Design §4.1 names the cost terms (creation-date
  equality, project/context overlap, normalised Levenshtein, line distance) without weights.
- Answer: sidecar mode is now the default, not an M10 opt-in (reverses
  `txtodo-implementation-plan.md` decision 9) — a plain, unmanaged todo.txt should work with
  `txtodo` with zero `id:` metadata written into it. Weights, v1/tunable:
  ```
  cost = 3.0 * date_mismatch(0 or 1)
       + 2.0 * (1 - jaccard(projects))
       + 1.0 * (1 - jaccard(contexts))
       + 6.0 * normalized_levenshtein(description)
       + 1.5 * (position_delta / max_task_count)
  MATCH_THRESHOLD = 5.0   // below the cost of "same everything, description fully rewritten" (6.0)
  ```
  Description gets the heaviest weight (strongest human-legible identity signal); the threshold
  sits just under "identical except description fully rewritten" (6.0) so a full rewrite is
  classified delete+insert (a visible duplicate) rather than risking a wrong merge — matches this
  design section's own stated preference for "resurrect as duplicate" over a bad merge. Lives as
  named constants in `crates/txtodo-model/src/identity.rs`; retune there directly, no ADR needed
  for a weight change alone.

## Q3 — Is `rec:` recurrence a core feature or a plugin?
- Status: answered 2026-09-13 · Raised: 2026-09-11 (plan §6.3) · Blocks: M10 plugin host scope
- Default until answered: **plugin** (design §9). The tag still parses in core (specs/todotxt.abnf `rec-tag`).
- Answer: plugin

## Q4 — iOS: accept a Local Network permission prompt for LAN MCP, or make iOS relay-only for agents?
- Status: answered 2026-09-13 · Raised: 2026-09-11 (plan §6.4) · Blocks: M9 iOS MCP transport
- Default until answered: undecided. LAN MCP on iOS needs `NSLocalNetworkUsageDescription` and Bonjour
  service declarations. Ref: https://developer.apple.com/documentation/bundleresources/information-property-list/nslocalnetworkusagedescription
- Answer: scope MCP to only the desktop versions (macOS, Linux, Windows). iOS relay-only for agents. The iOS agent will not be able to use MCP on LAN.

## Q5 — Relay hosting: does the project run a public relay, or self-host only?
- Status: open · Raised: 2026-09-11 (plan §6.5) · Blocks: M8 self-hosting docs, M9 push registration
- Default until answered: **self-host only**; docs describe running `relay/` yourself.
- Answer: The relay will be a hosted SaaS that I will grant users access to manually

## Q6 — Pairing: what happens when the initiator's and joiner's identity_mode disagree?
- Status: answered 2026-09-13 · Raised: 2026-09-13 (plan `floofy-swinging-brooks.md`, sidecar-identity Phase 2) ·
  Blocks: pairing inheriting identity_mode (a joining device otherwise decides its own, from
  whatever `id:` tags its own files already have — plan decision 3)
- Default until answered: **no propagation yet**. `load_or_mint_identity_mode` runs unconditionally
  in `Workspace::open_with_default_mode`, before pairing ever touches the workspace, so a joiner's
  mode is fixed by the time any pair RPC could adopt one.
- Context: `group_id` propagates today as a plaintext field on `PairOfferResponse`
  (`crates/txtodo-daemon/src/pairing_grpc.rs`), a meaningless random label a joiner blindly
  overwrites via `Workspace::adopt_group_key` — harmless, since nothing about the joiner's own
  state depended on its old value. `identity_mode` is not that: it is derived from real properties
  of the joiner's own files (`Tagged` iff a document already carries an `id:` tag, "no silent mode
  flip on upgrade" — `workspace.rs`'s `load_or_mint_identity_mode` doc) and `DocState`/`reconcile`
  assume it matches what is actually on disk. Blindly overwriting it the way `group_id` is
  overwritten could desync that invariant (e.g. a joiner with already-tagged files told to adopt
  `Sidecar` from the initiator). No wire message for the sealed group key exists yet either
  (`pairing_grpc.rs`'s own module doc: the daemon-to-daemon transport, `sync-lan-transport`,
  doesn't exist) — `PairingRegistry`/`Workspace::adopt_group_key` are a relay seam exercised only
  by `pairing_grpc_tests.rs` standing in for that transport.
- Needs a human decision, not just implementation: does a mode mismatch refuse the pairing outright
  (with what message), does the joiner defer to the initiator only when the joiner's own workspace
  has zero tasks yet (so there is nothing on disk to desync), or something else? Once decided, the
  wire part is small — a new field on `PairOfferResponse`, the same shape as `group_id`'s.
- Follow-up 2026-09-13 (agent, `tasks/sync-pairing`, `@cli` slice): built the wire part this
  question already named — `PairOfferResponse.identity_mode` (`crates/txtodo-proto/proto/txtodo/
  v1/txtodo.proto`) — and, without guessing the policy above, made `txtodo pair <code>` refuse a
  detected mismatch whenever the joiner's workspace already has tasks, proceeding unmodified (never
  adopting the initiator's mode) when modes match or the joiner is empty. The actual policy — what
  a real, non-empty mismatch should eventually do instead of just refusing — is still unanswered.
- Answer 2026-09-13: drop `Tagged` entirely. `Sidecar` becomes the only `identity_mode`. This
  question dissolves rather than resolves — with one mode, an initiator/joiner mismatch is no
  longer representable, so the mismatch-policy decision, the wire field, and the refusal check
  the 2026-09-13 follow-up added are all now dead code to remove, not logic to extend. Reverses
  Q2's "`Tagged` iff a document already carries an `id:` tag" framing — needs an ADR (line 129) and
  a pass over `load_or_mint_identity_mode`/`DocState`/`reconcile` (`workspace.rs`) plus
  `PairOfferResponse.identity_mode` (`txtodo-proto`) and the `txtodo pair` refusal path
  (`pairing_grpc.rs`) to remove the now-dead `Tagged` branch and mismatch check.

## Q7 — Is priority one global scale across workspaces, or per workspace?
- Status: answered 2026-09-13 · Raised: 2026-09-13 (board review; todo `desktop-universal-view`, `adr-global-daemon`)
  · Blocks: M11 universal view sort order, the `pri` operand in `txtodo-query`, ordering in the
  multi-workspace MCP gateway
- Default until answered: **global** — `(A)` in `+home` ranks with `(A)` in `+work`; the universal
  view sorts by (priority, created, workspace) and shows the owning workspace in the breadcrumb,
  which is what todo line 140 already says.
- Context: nothing today compares tasks across files; `crates/txtodo-cli/src/commands/list.rs`'s
  `sort_key` is per-file todo.sh order. Two readings of "universal priority interface":
  (a) one scale, projects are labels; (b) per-workspace scales plus a workspace weight (`+work`
  outranks `+home` on weekdays), which needs a saved-view or plugin concept (design §8 saved views,
  §9). Recommend (a) for v1, (b) later as a saved view over the query language, never as a core
  rule.
- Answer 2026-09-13: (a), global flat scale, as recommended. `(A)` ties within a single list are
  fine and expected — same-priority tasks across different projects/contexts in one workspace just
  sort equal, no forced tiebreak needed beyond (priority, created, workspace). Explicitly rejects
  any implicit cross-workspace weighting (e.g. "+work matters more on weekdays") as a core rule —
  workspaces stay separate entities the user switches between explicitly (GUI/TUI/CLI), so what's
  "high priority" is always relative to whichever workspace currently has the user's attention, not
  a computed global ranking. A future aggregation feature (surface top priorities across
  workspaces/lists/projects) is plausible but out of scope now — if built, it should be additive
  (a view over multiple workspaces the user opts into), not a change to how priority is scored. — Global daemon: one sync group per set of devices, or one per workspace?
- Status: open · Raised: 2026-09-13 · Blocks: `daemon-workspace-registry` (todo 130),
  `daemon-workspace-actor` (132: "each with its own store, op log, CRDT and sync Link"), the
  `sync-pairing` second-device handoff
- Default until answered: **per workspace**, as line 132 is written. Pairing N projects then means
  N `txtodo pair` runs and N group keys in the keystore.
- Context: `Workspace` owns the device id, key store and group key
  (`crates/txtodo-daemon/src/workspace.rs:35-54`, `adopt_group_key` at :339); ADR 0010 puts state
  under `<workspace>/.txtodo/`. "All your devices, all your projects" reads as pair once.
  Options: (a) one device identity and one group per device-set; a workspace is a namespace inside
  the group, ops carry a workspace id, one `Link` multiplexes; (b) per-workspace groups, pair each.
  Recommend (a): it is what "universal" means, and it is cheaper before M11's actor nesting than
  after. Either answer wants an ADR — line 129 is the place.

## Q9 — Pull `txtodo-query` (design §8) forward from M10 into M11?
- Status: open · Raised: 2026-09-13 · Blocks: the universal view's filter bar (todo 140), MCP
  `todo_search` (line 21: "substring-only, no query index yet"), a cross-workspace `txtodo ls`
- Default until answered: three ad-hoc filters keep growing (ABSTRACTIONS.md 2026-09-13, third
  entry); `crates/txtodo-query/src/lib.rs` stays two lines.
- Decide: (a) a v1 subset now — operands `done pri +project @context text key:value`, connectives
  `and or not`, no relative dates — behind `Query::parse` / `Query::matches(&Task)` / one
  `SortKey`, called by CLI, MCP and desktop; (b) wait for M10. Recommend (a). Sub-question: may
  v1 be a strict subset of the §8 grammar, or is §8 frozen as written (relative dates, `--explain`)?

## Q10 — Is "no two real txtodod processes in an agent session" a standing rule?
- Status: open · Raised: 2026-09-13 · Blocks: todo lines 8, 9 (integration half), 10, 11, 20 — each
  says "off-limits this session"
- Default until answered: agents keep skipping them; those five lines never close.
- Decide: (a) standing rule — then add a human-run `just sync-e2e` recipe and retag the five lines
  `@human`; (b) per-session — say so, and the `TwoDaemons` fixture (ABSTRACTIONS.md 2026-09-13,
  last entry) gets built with a bounded timeout and kill-on-drop. Recommend (b); (a) only if the
  worry is stray sockets or ports on your machine.

## Q11 — LAN transport: wire it into the daemon now for a two-host test, or skip to M8 relay?
- Status: open · Raised: 2026-09-13 · Blocks: `sync-lan-transport` (todo 2), the `sync-pairing`
  handoff (3), `test-nested-ref-sync` fresh-device half (20)
- Context: the M4 blocker is one `#[ignore]`d test (`crates/txtodo-sync/src/endpoint_tests.rs:74`)
  that forces both ends onto `127.0.0.1`; the production constructor `bind_local_endpoint` binds
  all interfaces and has never been exercised host to host. Separately, `Discovery`/`PeerTable`/
  `Link` are not referenced anywhere in `crates/txtodo-daemon/src` — the daemon wiring was "not
  attempted" (notes.md pass 3), so nothing end to end exists to run on two machines yet.
- Decide: (a) wire discovery + `Link` into `txtodod` behind a flag as the next `@sync` task, then
  you run the two-host check below and paste `doctor`'s output here; (b) leave LAN QUIC untested
  and make M8's file-carrier or relay the first real cross-device transport. Recommend (a): the
  wiring is unblocked work, and one real run either clears or confirms the upstream bug.
  Once (a) lands, the drivable check:
  ```bash
  # host A
  txtodo --dir ~/todo daemon start && txtodo --dir ~/todo pair
  # host B, same Wi-Fi, paste the code A printed
  txtodo --dir ~/todo pair <code>
  txtodo --dir ~/todo doctor   # expect one peer line, skew Ok
  ```
