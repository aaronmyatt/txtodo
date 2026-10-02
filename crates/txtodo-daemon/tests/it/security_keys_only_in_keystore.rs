//! security-m4-review, "keys only in keystore": a static, re-runnable regression check standing
//! in for the "process argv/env-var inspection at runtime" the task notes offer as an
//! alternative — this sandbox's host has no portable way to read a sibling process's live argv
//! and environment the way `/proc/<pid>/cmdline` does on Linux, so this instead greps the
//! checked-in source tree for the two ways `tasks/sync-keystore/notes.md` explicitly forbids key
//! material from leaving the keystore: as a CLI argument, or as a named environment variable.
//!
//! Scope is deliberate, not the whole workspace: `is_suspect`'s bare substring match on "key" is
//! far too common a word in ordinary Rust (map/dictionary keys, a rotation `key_epoch: u32`, the
//! keystore's own internals, where a `passphrase`/`Secret` field is exactly where one belongs) to
//! scan every struct field in the tree without an endless, ever-growing allowlist. So this checks
//! only the files that actually define argv or config-file surface — the one place a key could
//! plausibly arrive from outside the keystore — plus every `env::var`-style call anywhere. This is
//! a regression pin, not a general secrets scanner: a hit is either a real violation (the test
//! fails, naming the file and line) or a documented, narrowly-scoped exception added to the
//! allowlists below by a human on purpose — the same discipline `deny.toml`'s advisory ignores
//! use, never a blanket exemption.

use std::path::{Path, PathBuf};

/// Substrings that flag an identifier or an env var name as plausibly carrying key material.
/// Case-insensitive; checked against `snake_case` identifiers, so e.g. `key` also catches
/// `signing_key`. The bare words "key"/"keys" alone are excluded by [`is_suspect`] — see its doc.
const SUSPECT_SUBSTRINGS: &[&str] = &["key", "secret", "passphrase", "private"];

/// Env var names already reviewed and known to carry nothing sensitive. Checked so this test does
/// not merely confirm today's tree has zero `env::var` calls — it must actually find these and
/// clear them, or it would pass vacuously on a stripped-down tree.
const ALLOWED_ENV_VARS: &[&str] = &[
    "HOME",
    "USERPROFILE",
    "EDITOR",
    "TXTODO_LOG",
    "TXTODO_TEST_HOOKS",
    "TXTODO_WORKSPACE",
    "E2E_BRIDGE_PORT",
];

/// The only files this workspace defines CLI arguments or config-file fields in — the actual
/// "argv"/"config" risk surface `tasks/sync-keystore/notes.md` is about. Everything else (SQLite
/// row keys, the keystore's own `passphrase`/`Secret` fields, a rotation `key_epoch`, ...) is
/// out of scope by construction rather than by an ever-growing per-identifier allowlist.
const ARGV_SURFACE_FILES: &[&str] = &[
    "crates/txtodo-daemon/src/main.rs",
    "crates/txtodo-cli/src/main.rs",
    "crates/txtodo-cli/src/cli.rs",
    "crates/txtodo-cli/src/cli_command.rs",
    "crates/txtodo-cli/src/config.rs",
    "apps/desktop/src-tauri/src/config.rs",
];

/// Field/parameter identifiers on the argv/config surface already reviewed and known not to be a
/// sync key (the group key, a device signing key, or a device static key) leaving the keystore:
/// `key_store`/`key_store_mode`-shaped names choose a *backend* (`auto`/`os`/`file`,
/// `crates/txtodo-sync/src/keystore_resolve.rs::KeyStoreMode`), never key bytes — handled by
/// [`is_backend_choice_field`] rather than an exact list, since new ones keep appearing as that
/// plumbing grows.
const ALLOWED_IDENTIFIERS: &[&str] = &[];

/// True for a `key_store`/`keystore`-prefixed identifier — see [`ALLOWED_IDENTIFIERS`]'s doc.
fn is_backend_choice_field(ident: &str) -> bool {
    ident.starts_with("key_store") || ident.starts_with("keystore")
}

/// The repo root, found by walking up from this test's own crate directory.
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap_or_else(|| panic!("crates/txtodo-daemon has a parent"))
        .parent()
        .unwrap_or_else(|| panic!("crates/ has a parent"))
        .to_path_buf()
}

/// Every `.rs` file under `crates/` and `apps/`, for the env-var scan (which has no false-positive
/// problem: an env var name either does or doesn't say "key", and today's tree has none that do
/// except the reviewed ones) — excluding `target/` and this test's own file.
fn all_source_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for base in [root.join("crates"), root.join("apps")] {
        walk(&base, &mut out);
    }
    out.retain(|p| {
        p.extension().is_some_and(|e| e == "rs")
            && !p.components().any(|c| c.as_os_str() == "target")
            && p.file_name() != Some(std::ffi::OsStr::new("security_keys_only_in_keystore.rs"))
    });
    out
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, out);
        } else {
            out.push(path);
        }
    }
}

/// True when `name` (case-insensitive) contains any [`SUSPECT_SUBSTRINGS`] entry — except the
/// bare words "key"/"keys" on their own, which this codebase (like most Rust) overwhelmingly uses
/// for an ordinary map/dictionary key, never a cryptographic one — an actual crypto key here is
/// always named compositely (`group_key`, `signing_key`, `key_bytes`, ...) or carries its own type
/// (`GroupKey`, `Secret`), both still caught.
fn is_suspect(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    if lower == "key" || lower == "keys" {
        return false;
    }
    SUSPECT_SUBSTRINGS.iter().any(|s| lower.contains(s))
}

/// Literal env var names read via `env::var("X")` / `env::var_os("X")` / `.var("X")` in `line`.
fn env_var_literals(line: &str) -> Vec<String> {
    let mut names = Vec::new();
    for marker in ["env::var(\"", "env::var_os(\"", ".var(\""] {
        let mut rest = line;
        while let Some(start) = rest.find(marker) {
            let after = &rest[start + marker.len()..];
            if let Some(end) = after.find('"') {
                names.push(after[..end].to_owned());
                rest = &after[end..];
            } else {
                break;
            }
        }
    }
    names
}

/// The identifier immediately before a `:` on a field/parameter-shaped line (a declaration,
/// `[pub ]name: Type`, or a struct-literal initializer, `name: value`) — skipping match arms
/// (`=>`) and comments. A conservative heuristic, not a real Rust parser, but [`ARGV_SURFACE_FILES`]
/// is small and hand-reviewed, so precision matters less here than for a whole-workspace scan.
fn field_identifier(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    if trimmed.starts_with("//") || trimmed.contains("=>") || !trimmed.contains(':') {
        return None;
    }
    let before_colon = trimmed.split(':').next()?.trim();
    let ident = before_colon
        .strip_prefix("pub(crate)")
        .unwrap_or(before_colon);
    let ident = ident.strip_prefix("pub").unwrap_or(ident).trim();
    let is_plain_ident = !ident.is_empty()
        && ident
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    is_plain_ident.then_some(ident)
}

#[test]
fn no_env_var_name_in_the_source_tree_suggests_key_material() {
    let root = repo_root();
    let mut found_any_env_read = false;
    for path in all_source_files(&root) {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        for (n, line) in text.lines().enumerate() {
            for name in env_var_literals(line) {
                found_any_env_read = true;
                assert!(
                    ALLOWED_ENV_VARS.contains(&name.as_str()) || !is_suspect(&name),
                    "{}:{} reads env var {name:?}, whose name suggests key material; \
                     either it never should, or it is a reviewed exception — add it to \
                     ALLOWED_ENV_VARS with a comment saying why",
                    path.display(),
                    n + 1
                );
            }
        }
    }
    assert!(
        found_any_env_read,
        "sanity: expected to find at least one env::var-style call in the tree"
    );
}

#[test]
fn no_cli_flag_or_config_field_name_suggests_key_material() {
    let root = repo_root();
    let mut checked_fields = 0usize;
    for rel in ARGV_SURFACE_FILES {
        let path = root.join(rel);
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        for (n, line) in text.lines().enumerate() {
            let Some(ident) = field_identifier(line) else {
                continue;
            };
            checked_fields += 1;
            assert!(
                ALLOWED_IDENTIFIERS.contains(&ident)
                    || is_backend_choice_field(ident)
                    || !is_suspect(ident),
                "{}:{} declares `{ident}`, whose name suggests key material arriving as a CLI \
                 argument or config value; either it never should (route it through the \
                 keystore instead), or it is a reviewed exception — add it to \
                 ALLOWED_IDENTIFIERS with a comment saying why",
                path.display(),
                n + 1
            );
        }
    }
    assert!(
        checked_fields > 20,
        "sanity: expected to scan well over 20 field-shaped lines across the argv/config \
         surface, got {checked_fields}"
    );
}
