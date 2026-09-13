//! Where the relay's flags come from: argv, then environment variables, then the bounds
//! defaults (tasks/relay-reference/notes.md: "Config via env/flags: listen port, data dir,
//! retention; refuse to run without an explicit data dir rather than guessing"). Hand-rolled —
//! matching `txtodo-daemon`'s own `main.rs` arg loop rather than pulling in a flags crate no
//! other service binary in this workspace uses — so `--help`'s exact text is this module's own
//! [`HELP`] constant, which `docs/relay.md` is pinned against by
//! `tests/help_matches_docs.rs` (tasks/docs-relay-selfhost, acceptance: "relay --help output
//! matches every flag name docs/relay.md mentions").

use crate::bounds::{MAX_BLOB_SIZE, MAX_RETENTION_DAYS};
use std::net::SocketAddr;
use std::path::PathBuf;

/// Default bind address when `--listen`/`RELAY_LISTEN` is omitted.
const DEFAULT_LISTEN: &str = "127.0.0.1:8787";

/// The exact text `relay --help` prints — also what `tests/help_matches_docs.rs` and
/// `docs/relay.md` are both pinned against. Every flag name here must exist in that doc, and
/// every flag name in that doc must exist here (tasks/docs-relay-selfhost's drift test).
pub const HELP: &str = "\
relay [OPTIONS]

Reference relay (M8, design §4.5): stores ciphertext blobs it can't read, forwards push
wake-ups. It never parses or decrypts what it stores (design §4.6).

OPTIONS:
    --data-dir <PATH>          Directory holding the relay's SQLite store. Required — refused
                                rather than guessed. [env: RELAY_DATA_DIR]
    --listen <ADDR:PORT>       Address the HTTP surface binds to.
                                [env: RELAY_LISTEN] [default: 127.0.0.1:8787]
    --retention-days <N>       Days a blob is kept before the retention sweep removes it.
                                [env: RELAY_RETENTION_DAYS] [default: 30]
    --max-blob-bytes <N>       Largest ciphertext blob accepted per write.
                                [env: RELAY_MAX_BLOB_BYTES] [default: 262144]
    --help                     Print this help and exit.
";

/// Parsed configuration, or a request to print [`HELP`] and exit — `main.rs` matches on this
/// instead of every caller re-checking for `--help`.
#[derive(Debug)]
pub enum Action {
    /// Run the relay with this configuration.
    Run(Config),
    /// Print [`HELP`] and exit 0; nothing was started.
    Help,
}

/// The relay's resolved configuration. Every field but `data_dir` has a bounds-derived default;
/// `data_dir` has none — [`parse`] refuses to return a [`Config`] without one explicitly set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// Directory holding `relay.db`. Always explicit — never the cwd, never guessed.
    pub data_dir: PathBuf,
    /// Address the HTTP surface binds to.
    pub listen: SocketAddr,
    /// Days a blob is kept before the retention sweep removes it.
    pub retention_days: i64,
    /// Largest ciphertext blob accepted per write, in bytes.
    pub max_blob_bytes: usize,
}

/// Looks up one flag's value: `--name` on argv, else `env_name` in the environment.
struct Source<'a> {
    args: &'a std::collections::HashMap<String, String>,
    env: &'a dyn Fn(&str) -> Option<String>,
}

impl Source<'_> {
    fn get(&self, flag: &str, env_name: &str) -> Option<String> {
        self.args
            .get(flag)
            .cloned()
            .or_else(|| (self.env)(env_name))
    }
}

/// Parses `--data-dir`, `--listen`, `--retention-days`, `--max-blob-bytes`, `--help` from
/// `args` (excluding argv\[0\]); `env` resolves a variable by name (`std::env::var` in `main`,
/// a fake map in tests). Every flag also has an env-var fallback (see [`HELP`]); `--data-dir` /
/// `RELAY_DATA_DIR` is the one required value — everything else has a `bounds`-derived default.
pub fn parse(
    args: impl Iterator<Item = String>,
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<Action, String> {
    let mut values = std::collections::HashMap::new();
    let mut args = args.peekable();
    while let Some(flag) = args.next() {
        if flag == "--help" {
            return Ok(Action::Help);
        }
        let value = args.next().ok_or_else(|| format!("{flag} needs a value"))?;
        values.insert(flag, value);
    }
    let source = Source { args: &values, env };
    build_config(&source).map(Action::Run)
}

fn build_config(source: &Source<'_>) -> Result<Config, String> {
    let data_dir = source.get("--data-dir", "RELAY_DATA_DIR").ok_or(
        "refusing to start without an explicit data dir: pass --data-dir or set RELAY_DATA_DIR",
    )?;
    let listen = source
        .get("--listen", "RELAY_LISTEN")
        .unwrap_or_else(|| DEFAULT_LISTEN.to_owned());
    let listen = listen
        .parse::<SocketAddr>()
        .map_err(|e| format!("--listen {listen:?} is not ADDR:PORT: {e}"))?;
    let retention_days = parse_num(
        source,
        "--retention-days",
        "RELAY_RETENTION_DAYS",
        MAX_RETENTION_DAYS,
    )?;
    let max_blob_bytes = parse_num(
        source,
        "--max-blob-bytes",
        "RELAY_MAX_BLOB_BYTES",
        MAX_BLOB_SIZE,
    )?;
    Ok(Config {
        data_dir: PathBuf::from(data_dir),
        listen,
        retention_days,
        max_blob_bytes,
    })
}

fn parse_num<T: std::str::FromStr<Err = std::num::ParseIntError>>(
    source: &Source<'_>,
    flag: &str,
    env_name: &str,
    default: T,
) -> Result<T, String> {
    match source.get(flag, env_name) {
        None => Ok(default),
        Some(raw) => raw
            .parse::<T>()
            .map_err(|e| format!("{flag} {raw:?} is not a number: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_env(_name: &str) -> Option<String> {
        None
    }

    #[test]
    fn refuses_to_run_without_a_data_dir() {
        let err = parse(std::iter::empty(), &no_env).expect_err("no --data-dir, no RELAY_DATA_DIR");
        assert!(
            err.contains("data dir"),
            "error names the missing setting: {err}"
        );
    }

    #[test]
    fn help_flag_short_circuits_before_requiring_data_dir() {
        let args = vec!["--help".to_owned()].into_iter();
        assert!(matches!(parse(args, &no_env), Ok(Action::Help)));
    }

    #[test]
    fn flag_beats_env_beats_default() {
        let env = |name: &str| match name {
            "RELAY_DATA_DIR" => Some("/env/dir".to_owned()),
            "RELAY_LISTEN" => Some("127.0.0.1:9999".to_owned()),
            _ => None,
        };
        let args = vec!["--data-dir".to_owned(), "/flag/dir".to_owned()].into_iter();
        let Ok(Action::Run(config)) = parse(args, &env) else {
            panic!("expected Run with data_dir set by flag, listen by env")
        };
        assert_eq!(
            config.data_dir,
            PathBuf::from("/flag/dir"),
            "--data-dir beats RELAY_DATA_DIR"
        );
        assert_eq!(
            config.listen,
            "127.0.0.1:9999".parse().expect("valid addr"),
            "env beats default"
        );
        assert_eq!(
            config.retention_days, MAX_RETENTION_DAYS,
            "unset flag/env falls back to bounds default"
        );
        assert_eq!(config.max_blob_bytes, MAX_BLOB_SIZE);
    }

    #[test]
    fn rejects_a_non_numeric_retention_days() {
        let args = vec![
            "--data-dir".to_owned(),
            "/d".to_owned(),
            "--retention-days".to_owned(),
            "soon".to_owned(),
        ]
        .into_iter();
        let err = parse(args, &no_env).expect_err("non-numeric --retention-days");
        assert!(
            err.contains("--retention-days"),
            "error names the bad flag: {err}"
        );
    }

    #[test]
    fn help_text_documents_every_flag_it_accepts() {
        for flag in [
            "--data-dir",
            "--listen",
            "--retention-days",
            "--max-blob-bytes",
            "--help",
        ] {
            assert!(HELP.contains(flag), "HELP must mention {flag}");
        }
    }
}
