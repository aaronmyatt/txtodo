# Add a 60s cargo-fuzz smoke job on Linux to ci.yml

Plan M0: "GitHub Actions: … a 60 s fuzz smoke on Linux". Acceptance: `just fuzz parse_line 60` runs.

## Why nightly
cargo-fuzz drives libFuzzer through `-Z sanitizer`, which is nightly-only.
Ref: https://rust-fuzz.github.io/book/cargo-fuzz/setup.html

## Job sketch (ci.yml, frozen path: the fence asks)
```yaml
  fuzz:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@nightly
      - uses: Swatinem/rust-cache@v2
        with: { workspaces: "crates/txtodo-core/fuzz -> target" }
      - uses: taiki-e/install-action@cargo-fuzz
      - run: cargo +nightly fuzz run --fuzz-dir crates/txtodo-core/fuzz parse_line -- -max_total_time=60
```
Ref: https://github.com/taiki-e/install-action (has a `cargo-fuzz` alias).

## Gotchas
- 60 s wall clock, not iterations: `-max_total_time=60`.
- The fuzz crate is *not* a workspace member (see fuzz-stub-target); `--fuzz-dir` points at it.
- A crash writes `fuzz/artifacts/parse_line/crash-*`; upload it with `actions/upload-artifact` on failure.
- Keep the job out of the `check` matrix so a fuzz hiccup does not block the three-OS build.
