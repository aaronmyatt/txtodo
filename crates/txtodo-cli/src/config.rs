//! Where the files are: process environment (injected, never read below `main`), `config.toml`, and the
//! resolved [`Paths`]. Plan §1 decision 10 fixes the config location; plan M2 fixes the precedence.

use serde::Deserialize;
use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

/// The slice of the process environment the CLI reads. Built once in `main`, faked in tests.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Env {
    vars: BTreeMap<String, String>,
    cwd: PathBuf,
}

impl Env {
    /// From explicit variables and a working directory.
    pub fn new(vars: BTreeMap<String, String>, cwd: PathBuf) -> Env {
        debug_assert!(
            cwd.is_absolute() || cwd.as_os_str().is_empty(),
            "cwd is absolute"
        );
        Env { vars, cwd }
    }
    /// Snapshot of the real process. Non-UTF-8 values are dropped: no path here may be non-UTF-8 config.
    pub fn from_process() -> std::io::Result<Env> {
        let vars = std::env::vars_os()
            .filter_map(|(k, v)| Some((k.into_string().ok()?, v.into_string().ok()?)))
            .collect();
        Ok(Env::new(vars, std::env::current_dir()?))
    }
    fn var(&self, key: &str) -> Option<&str> {
        self.vars
            .get(key)
            .map(String::as_str)
            .filter(|v| !v.is_empty())
    }
    fn home(&self) -> Option<&str> {
        self.var("HOME").or_else(|| self.var("USERPROFILE"))
    }
    /// `~` and `~/x` expand against `$HOME` (`%USERPROFILE%`); relative paths resolve against the cwd.
    fn absolute(&self, p: &str) -> PathBuf {
        let expanded = match (p.strip_prefix('~'), self.home()) {
            (Some(rest), Some(home)) if rest.is_empty() || rest.starts_with('/') => {
                format!("{home}{rest}")
            }
            _ => p.to_string(),
        };
        self.cwd.join(expanded)
    }
}

/// How a workspace establishes task identity (docs/questions.md Q2). This crate's own copy —
/// `txtodo-model::IdentityMode` isn't a dependency this crate may take — but the same two values,
/// spelled the same way as the daemon's own `--identity-mode` flag.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdentityMode {
    /// Every task line carries an `id:<ULID>` tag.
    Tagged,
    /// No tags in the file; identity lives in the daemon's fingerprint index.
    Sidecar,
}

/// Which sync-keystore backend to resolve (plan M4 `sync-keystore`). This crate's own copy —
/// `txtodo-sync::KeyStoreMode` isn't a dependency this crate may take — spelled the same way as
/// the daemon's own `--key-store` flag.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyStoreMode {
    /// Prefer the OS backend; stop and ask rather than fall back to a file.
    Auto,
    /// OS keystore only. Unavailable is a hard error naming the reason.
    Os,
    /// Encrypted file only, chosen deliberately.
    File,
}

impl KeyStoreMode {
    /// The lowercase word `txtodo env` prints, matching the config/flag spelling.
    pub fn name(self) -> &'static str {
        match self {
            KeyStoreMode::Auto => "auto",
            KeyStoreMode::Os => "os",
            KeyStoreMode::File => "file",
        }
    }
}

/// `config.toml` as read from disk; every field optional (design §2.2 rule 4: absent means default).
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Directory holding todo.txt, done.txt and report.txt.
    pub todo_dir: Option<String>,
    /// Stamp `id:<ULID>` on `add`. Superseded by `identity_mode` when that is also set; kept
    /// working on its own for an existing config (docs/questions.md Q2 reverses plan §1 decision
    /// 9's old default of `true` — an unset config is `Sidecar` now, not `Tagged`).
    pub id_tags: Option<bool>,
    /// `"tagged"` or `"sidecar"` (docs/questions.md Q2); the name to reach for going forward.
    /// Unrecognised values fall back to `Sidecar`, same as leaving it unset.
    pub identity_mode: Option<String>,
    /// URL schemes recognised before tags; default `txtodo_core::urls::DEFAULT_SCHEMES`.
    pub url_schemes: Option<Vec<String>>,
    /// `"auto"` | `"os"` | `"file"` (plan M4 `sync-keystore`); unset or unrecognised is `"auto"`.
    /// Threaded to the daemon's own `--key-store` flag the same way `identity_mode` is threaded
    /// to `--identity-mode` (see that flag's own known gap: the service templates do not pass
    /// either through yet — a pre-existing limitation, not something this field introduces).
    pub key_store: Option<String>,
}

/// A config file that exists but cannot be used.
#[derive(Debug)]
pub struct ConfigError {
    path: PathBuf,
    message: String,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "cannot read config {}: {}",
            self.path.display(),
            self.message
        )
    }
}

impl Config {
    /// Parses the file at `path`; a missing file is the default config, anything else unreadable is an error.
    pub fn load(path: &Path) -> Result<Config, ConfigError> {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Config::default()),
            Err(e) => {
                return Err(ConfigError {
                    path: path.to_path_buf(),
                    message: e.to_string(),
                });
            }
        };
        toml::from_str(&text).map_err(|e| ConfigError {
            path: path.to_path_buf(),
            message: e.to_string(),
        })
    }
    /// Effective identity mode: `identity_mode` if set, else `id_tags` (`true` -> `Tagged`,
    /// `false` -> `Sidecar`) if THAT is set, else `Sidecar` (docs/questions.md Q2).
    pub fn identity_mode(&self) -> IdentityMode {
        if let Some(mode) = self.identity_mode.as_deref() {
            return match mode {
                "tagged" => IdentityMode::Tagged,
                _ => IdentityMode::Sidecar,
            };
        }
        match self.id_tags {
            Some(true) => IdentityMode::Tagged,
            Some(false) | None => IdentityMode::Sidecar,
        }
    }
    /// Effective sync-keystore backend mode (plan M4 `sync-keystore`); unset or unrecognised is
    /// `Auto`, the same "never fall back silently" default the daemon flag itself defaults to.
    pub fn key_store_mode(&self) -> KeyStoreMode {
        match self.key_store.as_deref() {
            Some("os") => KeyStoreMode::Os,
            Some("file") => KeyStoreMode::File,
            _ => KeyStoreMode::Auto,
        }
    }
    /// Effective `id_tags`, derived from `identity_mode()` when `id_tags` itself isn't set.
    pub fn id_tags(&self) -> bool {
        self.id_tags
            .unwrap_or_else(|| self.identity_mode() == IdentityMode::Tagged)
    }
    /// Effective URL schemes.
    pub fn url_schemes(&self) -> Vec<String> {
        self.url_schemes.clone().unwrap_or_else(|| {
            txtodo_core::urls::DEFAULT_SCHEMES
                .iter()
                .map(|s| s.to_string())
                .collect()
        })
    }
}

/// Every path the CLI touches, resolved once.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Paths {
    /// The todo directory.
    pub dir: PathBuf,
    /// `<dir>/todo.txt`.
    pub todo: PathBuf,
    /// `<dir>/done.txt`.
    pub done: PathBuf,
    /// `<dir>/report.txt`.
    pub report: PathBuf,
    /// The config file that was (or would have been) read.
    pub config: PathBuf,
}

/// `$TXTODO_CONFIG`, else `$XDG_CONFIG_HOME`, `%APPDATA%` or `~/.config`, then `txtodo/config.toml`.
pub fn config_path(env: &Env) -> PathBuf {
    if let Some(p) = env.var("TXTODO_CONFIG") {
        return env.absolute(p);
    }
    let base = env
        .var("XDG_CONFIG_HOME")
        .or_else(|| env.var("APPDATA"))
        .map(|b| env.absolute(b))
        .or_else(|| env.home().map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| env.cwd.clone());
    debug_assert!(
        base.is_absolute() || env.cwd.as_os_str().is_empty(),
        "base is absolute"
    );
    base.join("txtodo").join("config.toml")
}

/// `--dir` > `$TXTODO_TODO_DIR` > config `todo_dir` > cwd.
pub fn resolve(env: &Env, dir_flag: Option<&str>, config: &Config, config_file: PathBuf) -> Paths {
    let dir = dir_flag
        .or_else(|| env.var("TXTODO_TODO_DIR"))
        .or(config.todo_dir.as_deref())
        .map_or_else(|| env.cwd.clone(), |d| env.absolute(d));
    debug_assert!(
        dir.is_absolute() || env.cwd.as_os_str().is_empty(),
        "dir is absolute"
    );
    let paths = Paths {
        todo: dir.join("todo.txt"),
        done: dir.join("done.txt"),
        report: dir.join("report.txt"),
        dir,
        config: config_file,
    };
    debug_assert!(paths.todo.starts_with(&paths.dir), "files live in dir");
    paths
}
