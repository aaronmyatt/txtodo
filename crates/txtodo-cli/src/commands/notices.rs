//! `txtodo notices on|off|status` (ADR 0041, task daemon-notices): the daemon's own OS notice for
//! changes from another device. `off` leaves a marker file in the daemon's state dir (beside its
//! socket), `on` removes it; the daemon looks for it each time a notice is due, so no restart and
//! no RPC. The desktop's own notices have their own Settings toggle.

use std::path::{Path, PathBuf};

use crate::CliError;
use crate::config::Env;

/// The marker's name; `txtodo_daemon::notices::OFF_MARKER` (this crate may not depend on the
/// daemon, so the name is repeated).
const OFF_MARKER: &str = "notices-off";

/// `txtodo notices …`.
#[derive(Debug, Clone, Copy, clap::Subcommand)]
pub enum Action {
    /// The daemon shows a notice for changes from another device (the default).
    On,
    /// The daemon shows none.
    Off,
    /// Says which.
    Status,
}

/// The marker path for the daemon whose socket is `socket`.
pub fn marker(socket: &Path) -> PathBuf {
    socket.parent().unwrap_or(Path::new(".")).join(OFF_MARKER)
}

/// Runs `action` against the marker for this machine's daemon.
pub fn run(env: &Env, action: Action) -> Result<(), CliError> {
    let path = marker(&crate::config::global_socket_path(env));
    match action {
        Action::On => match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(CliError::Io(e)),
            _ => {}
        },
        Action::Off => {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir).map_err(CliError::Io)?;
            }
            std::fs::write(&path, b"").map_err(CliError::Io)?;
        }
        Action::Status => {}
    }
    let state = if path.exists() { "off" } else { "on" };
    println!("txtodo: daemon notices are {state}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_marker_sits_beside_the_socket() {
        assert_eq!(
            marker(Path::new("/data/txtodo/txtodod.sock")),
            PathBuf::from("/data/txtodo/notices-off")
        );
    }
}
