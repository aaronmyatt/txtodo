# Prometheus /metrics on 8637, OTel traces, Grafana dashboard in deploy/grafana (plan M10)

Goal: Prometheus metrics, OTel traces (one per sync session and reconciliation), Grafana dashboard JSON.

Design: txtodo-design.md §10 — metrics like `txtodo_tasks`, `txtodo_sync_lag_seconds`, `txtodo_reconcile_total`; dashboard in `deploy/grafana/`.

Plan: txtodo-implementation-plan.md M10 — mini-plan deferred until started.
