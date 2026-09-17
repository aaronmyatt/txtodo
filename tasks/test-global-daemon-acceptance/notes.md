# test-global-daemon-acceptance

## Summary

Tests: N-workspace daemon isolation (a crash in one doesn't affect others), registry survives
restart, clean-machine systemd/launchd single-unit install.

## As built

Real `txtodod` acceptance tests: a broken workspace's lazy-open failure never affects a healthy
one or the process (`a891387`); a genuinely separate second process against the same
`registry.db` re-opens everything the first registered (found and fixed a real test-harness bug
along the way: a `TempDir` owned by the first daemon deleted `registry.db` on drop).
Single-unit-install is already covered by `service.rs`'s own unit tests (`150821b`), not
duplicated here.
