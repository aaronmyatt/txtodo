//! Which build the app is, and which build the daemon it talks to is (task version-info). The
//! trigger, 2026-09-20: the installed app spawned a bundled `txtodod` from before early bind, and
//! nothing on screen said it was old. The frontend shows this app's version small and muted, and
//! warns when the daemon's version or release date differs (`$lib/versionInfo.ts`).

use crate::state::AppState;
use serde::Serialize;
use tauri::State;

/// This app's build, and the daemon's when it answers.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct BuildInfoDto {
    /// This app's version (the Cargo workspace version).
    pub version: String,
    /// This app's release date, `YYYY-MM-DD` or `unknown` (`build-support/buildinfo.rs`).
    pub release_date: String,
    /// The daemon's version; empty when it is not connected or did not answer.
    pub daemon_version: String,
    /// The daemon's release date; empty from a daemon older than `Health.release_date`.
    pub daemon_release_date: String,
}

/// The app's own half, which needs no daemon. `env!`: https://doc.rust-lang.org/std/macro.env.html
fn own_build() -> BuildInfoDto {
    BuildInfoDto {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        release_date: env!("TXTODO_RELEASE_DATE").to_owned(),
        daemon_version: String::new(),
        daemon_release_date: String::new(),
    }
}

/// Never fails: with no daemon the daemon half stays empty and the frontend shows only the app's
/// own version. The daemon-status banner already says the daemon is down.
#[tracing::instrument(name = "ipc.build_info", skip_all)]
#[tauri::command]
pub async fn build_info(state: State<'_, AppState>) -> Result<BuildInfoDto, String> {
    let mut info = own_build();
    if let Ok(mut client) = state.client_snapshot().await
        && let Ok((version, release_date)) = client.daemon_build().await
    {
        info.daemon_version = version;
        info.daemon_release_date = release_date;
    }
    Ok(info)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_apps_own_half_is_the_cargo_version_and_a_date_or_unknown() {
        let own = own_build();
        assert_eq!(own.version, env!("CARGO_PKG_VERSION"));
        assert!(
            own.release_date == "unknown" || own.release_date.len() == 10,
            "{}",
            own.release_date
        );
        assert!(own.daemon_version.is_empty() && own.daemon_release_date.is_empty());
    }
}
