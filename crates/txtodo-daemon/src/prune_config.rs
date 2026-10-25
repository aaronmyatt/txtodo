//! Where `PruneConfig` comes from (task `peer-prune`, ADR 0044 decision 2): `$TXTODO_PRUNE_PARKED_AFTER`
//! and `$TXTODO_PRUNE_QUIET_AFTER`, else the same keys, `prune_parked_after` and `prune_quiet_after`,
//! in the CLI's `config.toml`, else the defaults. The daemon reads the file itself: it is started by
//! a service with no flags, and the CLI does not hand it its config. A value is a duration (`7d`,
//! `12h`, `90m`, `30s`, bare digits are seconds) or the integer `0`, which turns that rule off; one
//! it cannot read is skipped, and logged by key, so a typo never switches pruning off by accident.

use std::path::PathBuf;

use crate::prune_stale::{PruneConfig, parse_duration_ms};

/// The process environment, injected so a test never reads the real one (the same idiom as
/// `txtodo-cli`'s `Env`).
pub(crate) type EnvFn<'a> = &'a dyn Fn(&str) -> Option<String>;

const PARKED_KEY: &str = "prune_parked_after";
const QUIET_KEY: &str = "prune_quiet_after";
const PARKED_ENV: &str = "TXTODO_PRUNE_PARKED_AFTER";
const QUIET_ENV: &str = "TXTODO_PRUNE_QUIET_AFTER";

/// `$TXTODO_CONFIG`, else `$XDG_CONFIG_HOME`, `%APPDATA%` or `~/.config`, then `txtodo/config.toml`:
/// the CLI's `config_path`, which this crate cannot depend on.
fn config_path(env: EnvFn<'_>) -> Option<PathBuf> {
    if let Some(path) = env("TXTODO_CONFIG") {
        return Some(PathBuf::from(path));
    }
    let base = env("XDG_CONFIG_HOME")
        .or_else(|| env("APPDATA"))
        .map(PathBuf::from)
        .or_else(|| env("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("txtodo").join("config.toml"))
}

/// The prune settings of this process: the real environment and the real config file.
pub(crate) fn load() -> PruneConfig {
    let env = |name: &str| std::env::var(name).ok();
    let text = config_path(&env).and_then(|path| {
        // A missing file is the usual case, not a problem.
        // Ref: https://doc.rust-lang.org/std/fs/fn.read_to_string.html
        std::fs::read_to_string(path).ok()
    });
    resolve(&env, text.as_deref())
}

/// Environment first, then the file's text, then the default, for each of the two settings.
pub(crate) fn resolve(env: EnvFn<'_>, file: Option<&str>) -> PruneConfig {
    // `toml::Table` is a TOML document read with no schema, so keys of the CLI's own are ignored.
    // Ref: https://docs.rs/toml/latest/toml/type.Table.html
    let table: Option<toml::Table> = file.and_then(|text| text.parse().ok());
    let default = PruneConfig::default();
    PruneConfig {
        parked_after_ms: pick(
            env,
            PARKED_ENV,
            table.as_ref(),
            PARKED_KEY,
            default.parked_after_ms,
        ),
        quiet_after_ms: pick(
            env,
            QUIET_ENV,
            table.as_ref(),
            QUIET_KEY,
            default.quiet_after_ms,
        ),
    }
}

fn pick(
    env: EnvFn<'_>,
    env_name: &str,
    table: Option<&toml::Table>,
    key: &'static str,
    default: u64,
) -> u64 {
    if let Some(raw) = env(env_name) {
        if let Some(ms) = parse_duration_ms(&raw) {
            return ms;
        }
        log_ignored(key, "env");
    }
    match table.and_then(|t| t.get(key)).map(file_value_ms) {
        Some(Some(ms)) => ms,
        Some(None) => {
            log_ignored(key, "file");
            default
        }
        None => default,
    }
}

/// A file value: a duration string, or an integer count of seconds.
fn file_value_ms(value: &toml::Value) -> Option<u64> {
    match value {
        toml::Value::String(text) => parse_duration_ms(text),
        toml::Value::Integer(secs) => u64::try_from(*secs).ok()?.checked_mul(1_000),
        _ => None,
    }
}

fn log_ignored(key: &'static str, source: &'static str) {
    tracing::warn!(key, source, "prune_config_value_ignored");
}

#[cfg(test)]
#[path = "prune_config_tests.rs"]
mod tests;
