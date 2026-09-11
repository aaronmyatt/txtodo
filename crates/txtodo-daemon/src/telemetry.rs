//! Structured logs (plan §5, tasks/daemon-tracing-logs): JSON lines to `.txtodo/logs/`, one file
//! per day, the newest `LOG_KEEP_FILES` kept. The `reconcile{file}` span wraps every external
//! change and apply; events carry ids, counts and hashes — never line text, tokens or payloads.
//! Ref: https://docs.rs/tracing · https://docs.rs/tracing-subscriber · https://docs.rs/tracing-appender

use std::path::Path;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// Daily files kept under `.txtodo/logs/`.
pub const LOG_KEEP_FILES: usize = 7;
/// File name prefix; tracing-appender adds `.YYYY-MM-DD`.
pub const LOG_FILE_PREFIX: &str = "txtodod.log";
/// Env var that overrides the default `info` filter (tracing-subscriber EnvFilter syntax).
pub const LOG_FILTER_ENV: &str = "TXTODO_LOG";

/// Keeps the non-blocking writer's buffer alive; drop it last (in `main`) or lines are lost.
pub struct LogGuard {
    _guard: tracing_appender::non_blocking::WorkerGuard,
}

/// Installs the global JSON subscriber writing under `logs_dir`. Call once.
pub fn init(logs_dir: &Path) -> Result<LogGuard, std::io::Error> {
    std::fs::create_dir_all(logs_dir)?;
    prune(logs_dir)?;
    // https://docs.rs/tracing-appender/latest/tracing_appender/rolling/fn.daily.html
    let file = tracing_appender::rolling::daily(logs_dir, LOG_FILE_PREFIX);
    // Bounded buffer (default 128k lines), lossy on overflow: logging never blocks an actor.
    let (writer, guard) = tracing_appender::non_blocking(file);
    let filter = tracing_subscriber::EnvFilter::try_from_env(LOG_FILTER_ENV)
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().json().with_writer(writer))
        .try_init()
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    debug_assert!(logs_dir.is_dir());
    Ok(LogGuard { _guard: guard })
}

/// Removes all but the newest `LOG_KEEP_FILES` rotated files. Dates sort lexically, so the file
/// name order is the age order. Returns how many were removed.
pub fn prune(logs_dir: &Path) -> Result<usize, std::io::Error> {
    let mut files: Vec<_> = std::fs::read_dir(logs_dir)?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(LOG_FILE_PREFIX))
        })
        .collect();
    files.sort();
    let excess = files.len().saturating_sub(LOG_KEEP_FILES);
    for old in &files[..excess] {
        std::fs::remove_file(old)?;
    }
    debug_assert!(files.len() - excess <= LOG_KEEP_FILES);
    Ok(excess)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prune_keeps_the_newest_seven_by_name() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        for day in 1..=10u8 {
            std::fs::write(
                dir.path()
                    .join(format!("{LOG_FILE_PREFIX}.2026-09-{day:02}")),
                b"{}\n",
            )
            .unwrap_or_else(|e| panic!("{e}"));
        }
        std::fs::write(dir.path().join("unrelated.txt"), b"").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(prune(dir.path()).unwrap_or_else(|e| panic!("{e}")), 3);
        let mut left: Vec<String> = std::fs::read_dir(dir.path())
            .map(|rd| {
                rd.flatten()
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        left.sort();
        assert_eq!(left.len(), LOG_KEEP_FILES + 1, "{left:?}");
        assert!(left[0] == "txtodod.log.2026-09-04", "{left:?}");
        assert_eq!(
            prune(dir.path()).unwrap_or_else(|e| panic!("{e}")),
            0,
            "idempotent"
        );
    }
}
