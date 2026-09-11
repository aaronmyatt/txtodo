# Prometheus /metrics on 8637, OTel traces, Grafana dashboard (plan M10, design §10)

## Goal

The daemon exposes Prometheus metrics on `127.0.0.1:8637` (ADR 0010), OpenTelemetry traces (one per
sync session and per reconciliation), and a Grafana dashboard JSON in `deploy/grafana/`. Design §10,
with the five metric families named verbatim: `txtodo_tasks{file,state,project}`,
`txtodo_sync_lag_seconds{peer}`, `txtodo_reconcile_total{outcome}`, `txtodo_conflicts_flagged_total`,
`txtodo_mcp_calls_total{tool,principal}`. The tracing spans already exist (plan §5, M3) — reuse them,
don't add a second instrumentation path.

## Design

```rust
// crates/txtodo-daemon/src/observability/metrics.rs
pub struct Metrics {
    pub tasks: prometheus::IntGaugeVec,              // txtodo_tasks{file,state,project}
    pub sync_lag: prometheus::GaugeVec,              // txtodo_sync_lag_seconds{peer}
    pub reconcile_total: prometheus::IntCounterVec,  // txtodo_reconcile_total{outcome}
    pub conflicts_flagged: prometheus::IntCounter,   // txtodo_conflicts_flagged_total
    pub mcp_calls: prometheus::IntCounterVec,        // txtodo_mcp_calls_total{tool,principal}
}
impl Metrics {
    pub fn register(r: &Registry) -> Result<Self, prometheus::Error>;
    pub fn record_tasks(&self, file: &str, state: &str, project: &str, n: i64);  // after each projection rebuild
    pub fn record_reconcile(&self, outcome: ReconcileOutcome);  // enum { Applied, Noop, Conflict }
    pub fn record_sync_lag(&self, peer: DeviceId, secs: f64);   // now - op.hlc_wall on each inbound batch
    pub fn record_mcp_call(&self, tool: &str, principal: &str); // in the MCP dispatch
    pub fn record_conflict_flagged(&self);                       // when M4 sets needs_review
}
pub async fn serve(metrics: Metrics, addr: SocketAddr) -> Result<(), std::io::Error>;  // axum GET /metrics, 127.0.0.1:8637
```

```rust
// crates/txtodo-daemon/src/observability/otel.rs
pub fn init_otlp(endpoint: &str) -> Result<tracing_opentelemetry::OpenTelemetryLayer<..>, OtelError>;
pub fn sync_session_span(peer: DeviceId) -> tracing::Span;   // "sync.session" — one per session
pub fn reconcile_span(file: &str) -> tracing::Span;          // "reconcile" — one per reconcile
```

- Instrumentation points are the existing plan §5 spans (`reconcile{file}`, `sync.session{peer}`,
  `mcp.call{tool,principal}`): the `tracing-opentelemetry` layer exports them as OTLP traces; the
  counter/gauge updates ride the same sites, so there is one instrumentation place, not two.
- `/metrics` is a tiny `axum` server (already a daemon dep via the M6 Streamable HTTP MCP) bound to
  `127.0.0.1:8637` only — non-loopback is refused by construction (security checklist, plan §5).
- OTLP endpoint from config (`otel_endpoint`, default unset = traces off) so local dev never requires
  a collector; `TXTODO_OTEL_ENDPOINT` env override.
- Grafana dashboard: `deploy/grafana/txtodo.json` — panels bound to the five families: "tasks completed
  per week by project" (design §10 names this panel verbatim), sync lag per peer, reconcile rate by
  outcome, conflicts flagged, MCP calls by tool/principal. Prometheus datasource `$datasource`. SLO
  annotations: convergence ≤ 2 s LAN / ≤ 30 s relay at 99.9 %, zero data loss.

## Placement/dependencies

- All Rust in `crates/txtodo-daemon/src/observability/{metrics.rs,otel.rs}` — `txtodo-daemon` is the
  only crate with a metrics/HTTP surface (decision 10 assigns the port). No workspace-edge change.
- New external deps need sign-off + `cargo deny check`: `prometheus`, `opentelemetry`,
  `opentelemetry-otlp`, `tracing-opentelemetry`, `opentelemetry_sdk`, `axum`.
- `deploy/grafana/` is a new dir — `deploy/` is NOT frozen (only `specs/**`, `.github/**`, the
  manifests, and `.claude/` are). No frozen path is touched by this task.

## Edge cases & invariants

- Metrics server must not block the actors: it owns its own tokio task; a scrape of 10 k tasks is
  cheap (pre-aggregated counters/gauges, no per-line iteration).
- Label cardinality is bounded (constitution: explicit max): `project` is top-N (≤ 20) + `(other)`;
  `peer`/`tool`/`principal` are bounded by device/token counts. Unbounded label values are rejected.
- `txtodo_tasks` is a gauge, not a counter: set to the full recount after each projection rebuild so a
  scrape is always consistent with the file.
- Invariant (assert the negative): no metric or span carries a task's description text or any token
  secret — only file names, counts, and principals (security checklist, plan §5).

## Acceptance

- `curl 127.0.0.1:8637/metrics` returns the five families in Prometheus text format with the exact
  names from design §10; binding `0.0.0.0:8637` is refused.
- An external edit reconciled → `txtodo_reconcile_total{outcome="applied"}` increments by 1; a no-op
  write → `outcome="noop"`; a conflict → `outcome="conflict"`.
- Two loopback daemons pair → `txtodo_sync_lag_seconds{peer}` is set and a `sync.session` OTLP trace
  is emitted (asserted with an in-process OTLP exporter in the test, no real collector).
- An MCP call increments `txtodo_mcp_calls_total{tool,principal}`; a `needs_review` flag increments
  `txtodo_conflicts_flagged_total`.
- `deploy/grafana/txtodo.json` parses as a Grafana dashboard and every panel's target references one
  of the five metric names (a small `serde_json` test walks the panels).
- `txtodo doctor --verbose` still dumps the last 100 events (plan §5) — unchanged by this task.

## References

- plan M10 + §5 (txtodo-implementation-plan.md), design §10 (txtodo-design.md)
- https://docs.rs/prometheus · https://docs.rs/opentelemetry · https://docs.rs/tracing-opentelemetry
- Prometheus exposition: https://prometheus.io/docs/instrumenting/exposition_formats/
- Grafana dashboard JSON: https://grafana.com/docs/grafana/latest/dashboards/build-dashboards/
