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

/// `config.toml` as read from disk; every field optional (design §2.2 rule 4: absent means default).
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Directory holding todo.txt, done.txt and report.txt.
    pub todo_dir: Option<String>,
    /// Stamp `id:<ULID>` on `add`. Default true (plan §1 decision 9).
    pub id_tags: Option<bool>,
    /// URL schemes recognised before tags; default `txtodo_core::urls::DEFAULT_SCHEMES`.
    pub url_schemes: Option<Vec<String>>,
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
    /// Effective `id_tags`.
    pub fn id_tags(&self) -> bool {
        self.id_tags.unwrap_or(true)
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
