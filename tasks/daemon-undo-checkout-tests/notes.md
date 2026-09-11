# Tests: undo restores bytes exactly and checkout renders the intermediate state (plan M3)

Plan M3 acceptance, bullets 3–4. These need control of time (checkout "between two ops") so the
daemon gets a guarded test seam rather than the tests sleeping across real millisecond boundaries.

## The seam
```toml
[features]
test-seam = []          # never in default features; CI builds release without it
[dev-dependencies]
txtodo-daemon = { path = ".", features = ["test-seam"] }   # integration tests get it
```
Under `cfg(feature = "test-seam")` the binary accepts `--fake-clock-ms <u64>` and installs a
`FakeClock` behind the injected `Clock` trait; a test-only `AdvanceClock` RPC moves it. Without the
feature the flag does not exist and the RPC returns `Unimplemented`. The human can drive the same
seam by hand (a checked-in `.http`-style `grpcurl` snippet lives in `tests/support/README.txt`).
Guarding is the point: it can never ship enabled (CLAUDE.md §2.3).

## Undo exactness
The external edit changes a description and a priority; undo must restore *bytes*, not just
fields — so the assertion is on the whole file, including the id tags and endings. Redo via
undo-of-undo is asserted the same way.

## Checkout between ops
Ops at fake time 1 000 and 2 000; checkout at `1 500` renders op-1 state. Checkout at `2 000`
includes op 2 (inclusive upper bound, see tasks/cli-history). Both go through the CLI so the
RFC 3339 conversion is covered; the fake clock's epoch is chosen so local-zone conversion is
unambiguous (a date well away from DST changes).
