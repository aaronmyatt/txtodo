# CI_DEPRECATIONS.patch.md — Node 20 actions onto Node 24 (2026-09-27)

Backlog: `tasks/ci-runner-deprecations/todo.txt` line 2. `.github/**` is frozen
(`.claude/budgets.json` `slices.frozenPaths`), so a human applies this, as with
`RELEASE_CI.patch.md`.

## What it changes

25 lines across `.github/workflows/ci.yml` and `release.yml`. Nothing but the version tags.

| Action | Uses | From | To | Why this major |
|---|---|---|---|---|
| `actions/checkout` | 15 | v4 | v5 | first major on Node 24 |
| `actions/setup-node` | 2 | v4 | v5 | first major on Node 24 |
| `actions/upload-artifact` | 6 | v4 | **v6** | v5 still runs on Node 20 |
| `actions/download-artifact` | 2 | v4 | **v7** | v5 and v6 still run on Node 20 |

- Not v5 across the board: `upload-artifact@v5` and `download-artifact@v5` still declare
  `using: node20`, so the Node 20 annotation would stay. Checked 2026-09-27 against each tag's
  `action.yml` and release notes:
  - https://github.com/actions/upload-artifact/releases/tag/v6.0.0
  - https://github.com/actions/download-artifact/releases/tag/v7.0.0
  - https://github.com/actions/checkout/releases/tag/v5.0.0
  - https://github.com/actions/setup-node/releases/tag/v5.0.0
- Not the newest majors either (checkout v7, setup-node v7, upload-artifact v7,
  download-artifact v8 exist). The smallest step that clears Node 20 is the one to review.
- Breaking changes checked against how this repo uses them:
  - All four need runner 2.327.1 or newer. GitHub-hosted runners already are.
  - `download-artifact@v5` changed the path for one artifact downloaded by `artifact-ids`. Both
    uses here download every artifact by `path: artifacts` only, so the layout stays
    `artifacts/<name>/`.
  - `setup-node@v5` caches on its own when the repo root's `package.json` has a
    `packageManager` field. There is no root `package.json`, and `apps/desktop/package.json` has
    no such field, so nothing changes.
- Left out: the runner question (pin `ubuntu-24.04` or ride `ubuntu-latest` into Ubuntu 26 from
  2026-10-19). That is the `@human` decide line 1; this patch does not touch any `runs-on`.

## Apply

From the repo root. The first command checks, the second applies:

```bash
sed -n '/^```diff$/,/^```$/p' CI_DEPRECATIONS.patch.md | sed '1d;$d' | git apply --check
```

```bash
sed -n '/^```diff$/,/^```$/p' CI_DEPRECATIONS.patch.md | sed '1d;$d' | git apply
```

Then check that no v4 of the four is left (prints the "none left" line on success):

```bash
! grep -nE 'actions/(checkout|setup-node|upload-artifact|download-artifact)@v4' .github/workflows/*.yml && echo "none left"
```

Commit, push, and watch one `ci` run and one `release` run for a Node 20 annotation: that is
line 3 of the sub-backlog.

## The diff

```diff
diff --git a/.github/workflows/ci.yml b/.github/workflows/ci.yml
index 2f20e26..ab0bbdb 100644
--- a/.github/workflows/ci.yml
+++ b/.github/workflows/ci.yml
@@ -33,7 +33,7 @@ jobs:
         os: [ubuntu-latest, macos-latest, windows-latest]
     runs-on: ${{ matrix.os }}
     steps:
-      - uses: actions/checkout@v4
+      - uses: actions/checkout@v5
         with:
           fetch-depth: 0   # PR size check needs the base commit
       # rust-toolchain.toml pins 1.95.0 with rustfmt + clippy; the action honours it.
@@ -111,7 +111,7 @@ jobs:
     runs-on: ubuntu-latest
     timeout-minutes: 25
     steps:
-      - uses: actions/checkout@v4
+      - uses: actions/checkout@v5
       - uses: dtolnay/rust-toolchain@stable
 
       # Install system packages needed to build glib-sys on the daemon job too
@@ -129,7 +129,7 @@ jobs:
     # core must build without std (plan M1): a target with no OS proves no std:: path leaked in
     runs-on: ubuntu-latest
     steps:
-      - uses: actions/checkout@v4
+      - uses: actions/checkout@v5
       - uses: dtolnay/rust-toolchain@stable
       # `targets:` on the action would land on `stable`, but rust-toolchain.toml overrides the
       # build to 1.95.0 (E0463 in CI 2026-09-11). rustup target add follows the override.
@@ -142,7 +142,7 @@ jobs:
     # perf budget (plan §5): parse 100k lines ≤ budgets.json.perf.parse100kMs on this runner
     runs-on: ubuntu-latest
     steps:
-      - uses: actions/checkout@v4
+      - uses: actions/checkout@v5
       - uses: dtolnay/rust-toolchain@stable
       - uses: Swatinem/rust-cache@v2
       - run: .claude/scripts/check-bench.sh
@@ -150,7 +150,7 @@ jobs:
   coverage:
     runs-on: ubuntu-latest
     steps:
-      - uses: actions/checkout@v4
+      - uses: actions/checkout@v5
       - uses: dtolnay/rust-toolchain@stable
         with:
           components: llvm-tools-preview
@@ -179,7 +179,7 @@ jobs:
   deny:
     runs-on: ubuntu-latest
     steps:
-      - uses: actions/checkout@v4
+      - uses: actions/checkout@v5
       # https://github.com/EmbarkStudios/cargo-deny-action
       - uses: EmbarkStudios/cargo-deny-action@v2
 
@@ -188,7 +188,7 @@ jobs:
     # https://rust-fuzz.github.io/book/cargo-fuzz/setup.html
     runs-on: ubuntu-latest
     steps:
-      - uses: actions/checkout@v4
+      - uses: actions/checkout@v5
       - uses: dtolnay/rust-toolchain@nightly
       - uses: Swatinem/rust-cache@v2
         with:
@@ -201,7 +201,7 @@ jobs:
         run: cargo +nightly fuzz run --fuzz-dir crates/txtodo-core/fuzz parse_line -- -max_total_time=60
       - name: keep any crash input
         if: failure()
-        uses: actions/upload-artifact@v4
+        uses: actions/upload-artifact@v6
         with:
           name: fuzz-artifacts
           path: crates/txtodo-core/fuzz/artifacts
@@ -224,7 +224,7 @@ jobs:
   desktop:
     runs-on: ubuntu-latest
     steps:
-      - uses: actions/checkout@v4
+      - uses: actions/checkout@v5
       - name: Install system dependencies (Ubuntu, Tauri Linux prerequisites)
         run: |
           sudo apt-get update
@@ -233,7 +233,7 @@ jobs:
       # apps/desktop/package.json has no "engines" field; Node 22 matches the version already
       # pinned in this same workflow file's sibling release.yml's build-desktop job, rather than
       # introducing a second Node version for this one app.
-      - uses: actions/setup-node@v4
+      - uses: actions/setup-node@v5
         with:
           node-version: '22'
       - name: npm ci
diff --git a/.github/workflows/release.yml b/.github/workflows/release.yml
index 11255cf..4e2c6cf 100644
--- a/.github/workflows/release.yml
+++ b/.github/workflows/release.yml
@@ -27,7 +27,7 @@ jobs:
   reject-duplicate-tag:
     runs-on: ubuntu-latest
     steps:
-      - uses: actions/checkout@v4
+      - uses: actions/checkout@v5
       - name: fail if this tag already has a published release
         env:
           GH_TOKEN: ${{ github.token }}
@@ -102,7 +102,7 @@ jobs:
             static_check: false
     runs-on: ${{ matrix.os }}
     steps:
-      - uses: actions/checkout@v4
+      - uses: actions/checkout@v5
       # The date every binary shows beside its version (task version-info). The tagged commit's
       # own date, so two builds of one tag agree; build-support/buildinfo.rs reads the variable.
       # https://docs.github.com/en/actions/reference/workflow-commands-for-github-actions#setting-an-environment-variable
@@ -234,7 +234,7 @@ jobs:
             esac
           done
 
-      - uses: actions/upload-artifact@v4
+      - uses: actions/upload-artifact@v6
         with:
           name: dist-${{ matrix.leg }}
           path: dist/*
@@ -264,7 +264,7 @@ jobs:
             native: false
     runs-on: macos-latest
     steps:
-      - uses: actions/checkout@v4
+      - uses: actions/checkout@v5
       # The date every binary shows beside its version (task version-info). The tagged commit's
       # own date, so two builds of one tag agree; build-support/buildinfo.rs reads the variable.
       # https://docs.github.com/en/actions/reference/workflow-commands-for-github-actions#setting-an-environment-variable
@@ -273,7 +273,7 @@ jobs:
         run: echo "TXTODO_RELEASE_DATE=$(git show -s --format=%cs HEAD)" >> "$GITHUB_ENV"
       - uses: dtolnay/rust-toolchain@stable
       - uses: Swatinem/rust-cache@v2
-      - uses: actions/setup-node@v4
+      - uses: actions/setup-node@v5
         with:
           node-version: '22'
       - name: rustup target add
@@ -334,7 +334,7 @@ jobs:
         if: always()
         run: |
           if [ -n "${TXTODO_SIGN_KEYCHAIN:-}" ]; then security delete-keychain "$TXTODO_SIGN_KEYCHAIN" || true; fi
-      - uses: actions/upload-artifact@v4
+      - uses: actions/upload-artifact@v6
         with:
           name: dist-desktop-${{ matrix.leg }}
           path: dist/*
@@ -344,7 +344,7 @@ jobs:
     needs: reject-duplicate-tag
     runs-on: ubuntu-latest
     steps:
-      - uses: actions/checkout@v4
+      - uses: actions/checkout@v5
       - uses: dtolnay/rust-toolchain@stable
       - uses: Swatinem/rust-cache@v2
       - run: rustup target add wasm32-unknown-unknown
@@ -373,7 +373,7 @@ jobs:
         run: curl https://rustwasm.github.io/wasm-pack/installer/init.sh -sSf | sh
       - name: build txtodo-ffi (wasm32)
         run: wasm-pack build crates/txtodo-ffi --release --target web --out-dir ../../dist-wasm
-      - uses: actions/upload-artifact@v4
+      - uses: actions/upload-artifact@v6
         with:
           name: dist-wasm
           path: dist-wasm/*
@@ -390,7 +390,7 @@ jobs:
     needs: reject-duplicate-tag
     runs-on: ubuntu-latest
     steps:
-      - uses: actions/checkout@v4
+      - uses: actions/checkout@v5
       - uses: dtolnay/rust-toolchain@stable
       - uses: Swatinem/rust-cache@v2
       # https://github.com/CycloneDX/cyclonedx-rust-cargo
@@ -419,7 +419,7 @@ jobs:
             jq -e --arg c "$c" '.components[] | select(.name == $c)' bom.json > /dev/null
           done
           jq -e '[.components[] | select(.licenses == null or .licenses == [])] | length == 0' bom.json
-      - uses: actions/upload-artifact@v4
+      - uses: actions/upload-artifact@v6
         with:
           name: bom
           path: bom.json
@@ -434,7 +434,7 @@ jobs:
     needs: reject-duplicate-tag
     runs-on: ubuntu-latest
     steps:
-      - uses: actions/checkout@v4
+      - uses: actions/checkout@v5
       # https://github.com/cachix/install-nix-action
       - uses: cachix/install-nix-action@v27
         with:
@@ -461,7 +461,7 @@ jobs:
       id-token: write
       contents: read
     steps:
-      - uses: actions/download-artifact@v4
+      - uses: actions/download-artifact@v7
         with:
           path: artifacts
       # https://github.com/sigstore/cosign-installer
@@ -474,7 +474,7 @@ jobs:
           find artifacts -type f ! -name '*.bundle' | while read -r f; do
             cosign sign-blob --yes --bundle "${f}.bundle" "$f"
           done
-      - uses: actions/upload-artifact@v4
+      - uses: actions/upload-artifact@v6
         with:
           name: signatures
           path: artifacts/**/*.bundle
@@ -490,8 +490,8 @@ jobs:
     permissions:
       contents: write
     steps:
-      - uses: actions/checkout@v4
-      - uses: actions/download-artifact@v4
+      - uses: actions/checkout@v5
+      - uses: actions/download-artifact@v7
         with:
           path: artifacts
       # Missing on this project's first real release run: this job runs on its own fresh
```
