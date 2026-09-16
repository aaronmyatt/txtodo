# txtodo-mcp: the mcp.call{tool,principal} span (root todo.txt line 36)

## Goal

`crates/txtodo-mcp` declares `tracing = "0.1"` with the comment "structured logs for the MCP
surface (matches txtodo-daemon's setup)" but has zero `tracing::` call sites and no wired
subscriber — an unused dependency with stated intent. Root todo 36 asks for exactly the span plan
§5 (`txtodo-implementation-plan.md:447`) already names: `mcp.call{tool,principal}`, JSON file +
stderr sinks, never stdout (the stdio transport owns stdout).

## Design

### Where the span goes

`schema.rs`'s `McpServer` has 15 `#[tool]` methods (`todo_list` … `todo_notes_set`), each a
one-line delegation to `tools_read.rs`/`tools_write.rs`. This — not `tools_read`/`tools_write` — is
the real tool-dispatch site: it is the only place that knows the *literal* MCP tool name for every
call, including the one case where two tools share one backend function
(`todo_complete`/`todo_uncomplete` both call `tools_write::complete`, distinguished only by the
`done` bool `schema.rs` passes in). Each method gets its own
`#[tracing::instrument(name = "mcp.call", skip_all, fields(tool = "<literal>", principal =
%self.backend.principal()))]`.

### Macro ordering (verified by compiling, not assumed)

`#[tracing::instrument]` is listed *above* `#[tool]`, never below. `#[tool]` (rmcp-macros'
`tool.rs`) rewrites an `async fn` into a sync fn returning `Pin<Box<dyn Future>>` by boxing the
original body; `#[rmcp::tool_router]` (on the enclosing `impl` block) never touches individual
method attributes, it only reads which methods still carry a `#[tool]` attribute to build routing
table entries referencing `Self::<method>` by name. Attribute macros stacked on one item expand
top-down: with `instrument` on top, it runs first, while the item is still the original `async
fn`, wraps its body in the span, and re-emits the function with `#[tool(...)]` still attached for
the compiler to expand next. Written the other way round, `#[tool]` would run first and
`instrument` would end up spanning a *sync* function that just returns an unstarted, boxed future
— the span would open and immediately close without ever covering the tool's real work. Confirmed
by building one method both ways; only `instrument`-above-`tool` compiles and behaves correctly
(`cargo check -p txtodo-mcp --all-targets`).

### `principal`

Added `fn principal(&self) -> String` to the `McpBackend` trait (sync — no I/O needed).
`GrpcMcpBackend::principal` reads `ctx.agent: Option<pb::AgentPrincipal { token_id, name }>` and
formats `"agent:<name>#<token_id>"`, or `"user"` when unset (today's default — no real token
principal exists yet, `mcp-agent-principal` is a separate future task per `grpc_backend.rs`'s own
doc). `token_id` is *not* a bearer secret: `crates/txtodo-daemon/src/tokens.rs::parse_token_id`
parses it as a `TokenId` ULID — the stable identifier `Store::verify_token` looks up by — not the
bearer string returned once, in plaintext, at token creation and never stored or logged anywhere
else in the codebase. Logging it here is the same category of thing `OpSummary::principal`
(`backend.rs`) already returns over the wire today (`"agent:name@dev"`).

### Sink matrix (stdout safety)

`transport.rs::serve_stdio` calls `rmcp::transport::io::stdio()` (`<https://docs.rs/rmcp/latest/
rmcp/transport/io/fn.stdio.html>`), which is stdin+stdout — confirmed by reading the function
before touching anything, per the task brief. `txtodo_telemetry::init("txtodo-mcp", logs_dir)`
installs a JSON rolling-file layer plus a *stderr* pretty layer (`crates/txtodo-telemetry/src/
lib.rs`) — never stdout, already built and already proven crate-locally (`logging-telemetry-crate`,
`logging-daemon-boot`). Wired at the top of `main.rs::run`, once per process, logs under
`<workspace>/.txtodo/logs/` — the same directory `txtodod` already writes its own
`txtodod.log.YYYY-MM-DD` family to, this binary's own `txtodo-mcp.log.YYYY-MM-DD` family
alongside it (one directory, one `service` field per line to tell processes apart, `txtodo doctor`
already reads this pattern for `txtodod`).

## Placement

- `crates/txtodo-mcp/Cargo.toml`: `txtodo-telemetry = { path = "../txtodo-telemetry" }` (prod dep);
  `tracing-subscriber = { workspace = true }` (dev-dep, test-only — see Acceptance).
- `.claude/budgets.json`: `slices.allowedDeps."txtodo-mcp"` gains `"txtodo-telemetry"` (one line;
  `.claude/UNFROZEN` is present, so this frozen path is writable this session).
- `crates/txtodo-mcp/src/backend.rs`: `McpBackend::principal(&self) -> String` added to the trait.
- `crates/txtodo-mcp/src/grpc_backend.rs`: `GrpcMcpBackend::principal` implementation.
- `crates/txtodo-mcp/src/schema.rs`: one `#[tracing::instrument(name = "mcp.call", ...)]` per
  `#[tool]` method (15 total), module doc explaining the ordering.
- `crates/txtodo-mcp/src/main.rs::run`: `txtodo_telemetry::init("txtodo-mcp", &args.dir.join(
  ".txtodo/logs"))?` at the top, `_log_guard` held for the function's whole body.
- `crates/txtodo-mcp/tests/smoke.rs`: `FakeBackend::principal` (test double, returns `"user"`),
  plus the proof test (see Acceptance).

## Edge cases

- `todo_complete`/`todo_uncomplete` share one backend fn (`tools_write::complete`) but are two
  separate `#[tool]` methods in `schema.rs`, so each gets its own literal `tool` field — the span
  correctly distinguishes them even though the code they call does not know its own tool name.
- `todo_raw` write mode carries line *text* as a tool argument (`RawArgs.text`) — never recorded;
  `skip_all` means the instrument macro never auto-records any argument, matching the repo-wide
  rule (`#[instrument(skip_all)]` always, per this task's own brief).
- `--http`/`--lan` mode never touches stdout either (`serve_http` binds a TCP listener + axum
  router) — the stdout constraint is stdio-mode-specific, but `telemetry::init` is unconditional
  (before the `match args.mode` branch) since neither mode may use stdout for logs.
- The `_log_guard` binding must outlive every `tracing::` call in `run` — it is a local `let`
  covering the function's remaining body (same shape `txtodo-daemon/src/main.rs::prepare_and_
  announce` uses for its own `_logs` guard), never dropped early.

## Acceptance

- The span's name is exactly `mcp.call`, fields exactly `tool`/`principal`, for all 15 tools.
- `principal` never contains a bearer secret; no tool argument, task line text, or payload is ever
  recorded on the span (`fields` lists only the two required keys; `skip_all` blocks the rest).
- A real, running test proves the JSON shape — not just "it compiles" (see As built).
- No write to stdout anywhere in touched code paths.
- `cargo fmt -p txtodo-mcp -- --check`, `cargo clippy -p txtodo-mcp --all-targets -- -D warnings`,
  `cargo test -p txtodo-mcp` green; no `#[allow]`/`#[expect]` anywhere new.

## As built (2026-09-16, agent)

Built exactly to the design above. `schema.rs`'s attribute lines needed manual multi-line
wrapping (`rustfmt` does not rewrap `#[tracing::instrument(...)]`'s nested `fields(...)` call to
the 100-col `lineWidth` budget on its own — confirmed empirically, `rustfmt --check` reported no
diff even at 128 columns) to stay under the repo's `lineWidth` budget; one pre-existing line in
`todo_move`'s `#[tool(description = "...")]` was already over budget before this task and was left
alone (out of scope; `todo_batch`'s multi-line `#[tool(...)]` was the existing precedent this
follows).

### Deviations from the plan

- None structural. The one thing genuinely discovered rather than assumed: the `#[tracing::
  instrument]`-above-`#[tool]` ordering — this was verified by actually compiling both orderings,
  not inferred from documentation, since `rmcp-macros`' `tool.rs`/`tool_router.rs` source
  (`~/.cargo/registry/.../rmcp-macros-3.3.0/src/`) was read directly to confirm `#[tool_router]`
  never rewrites a method's own attribute list, only reads it.

### Verification

- `cargo fmt -p txtodo-mcp -- --check`: clean.
- `cargo clippy -p txtodo-mcp --all-targets -- -D warnings`: clean — no `cognitive_complexity` hit
  on any of the 15 (already-trivial one-line) tool methods; no `#[allow]`/`#[expect]` anywhere new.
- `cargo test -p txtodo-mcp`: 20 lib tests + 4 integration tests (`tests/smoke.rs`, one new) green.
- New test `mcp_call_span_names_tool_and_records_principal` (`tests/smoke.rs`): builds its own
  `tracing_subscriber::registry().with(fmt::layer().json().with_span_events(FmtSpan::CLOSE)
  .with_writer(txtodo_telemetry::testing::LogSink::new()))` (`capturing_dispatch` alone doesn't
  enable span-close events, so a span with no event inside it never reaches the writer — this test
  needed the one thing that does), drives a real in-process `McpServer` over a duplex pipe (the
  same `connect()` helper `tool_list_is_the_exact_set` uses), calls `todo_list`, then parses the
  captured JSON line and asserts `span.name == "mcp.call"`, `span.tool == "todo_list"`,
  `span.principal == "user"` — and that no `token_id` text ever appears in the captured output.
  `#[tokio::test]`'s default current-thread runtime is load-bearing: the server side of `connect()`
  runs inside a `tokio::spawn`ed task, but since everything is polled on the one OS thread the
  `set_default` guard is a thread-local on, the span constructed inside that spawned task still
  sees the test's custom subscriber.
- `.claude/scripts/check-boundaries.sh`, `.claude/scripts/check-file-length.sh`: clean.
- Stdout: `grep -rn "print!\|println!\|io::stdout" crates/txtodo-mcp/src/*.rs` — zero hits. The
  only `std::io` writes in this crate's `main.rs` are three pre-existing `eprintln!`s (stderr,
  `#![allow(clippy::print_stderr)]` already covers them, predates this task).
- `cargo check -p txtodo-mcp --all-targets`: clean after the `principal` trait method landed on
  both real implementers (`GrpcMcpBackend`) and both test doubles that implement `McpBackend`
  (`tests/smoke.rs::FakeBackend`).

### Deliberately out of scope

- Any other `+m11 @observability` backlog line, or any file outside `crates/txtodo-mcp/`,
  `.claude/budgets.json` (one line), `tasks/logging-mcp-call-span/`, and this one root todo.txt
  line.
- A `cargo clippy -p txtodo-store`/`cargo build -p txtodo-store` failure
  (`projections.rs::put_projection`/`put_snapshot`, cognitive complexity 11/10) surfaced
  repeatedly from the PostToolUse feedback hook while editing this crate. Confirmed unrelated and
  pre-existing: `git stash && cargo build -p txtodo-store` and `cargo clippy -p txtodo-store
  --all-targets -- -D warnings` both pass clean on unmodified `main` (this task never touches
  `txtodo-store`); `tasks/logging-model/notes.md`'s own As-built section hit and logged the exact
  same pre-existing failure independently. Left unfixed — not this task's crate.
- `mcp-agent-principal` (real bearer-token-backed principals over the wire) — `principal()` today
  only formats whatever `GrpcMcpBackend::connect_unix`'s existing `agent` arg already carries;
  no new authentication was added.
