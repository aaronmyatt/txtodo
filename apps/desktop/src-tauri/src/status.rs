//! Daemon connectivity state surfaced to the Svelte frontend (design §5): daemon-absent or
//! disconnected renders as a reconnect banner, never a crash.

use serde::Serialize;

/// Connectivity state of the bridge to `txtodod`. Queryable via the `daemon_status` command and
/// pushed as a `daemon-status` event on every change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DaemonStatus {
    /// A live connection answered `Health`.
    Connected,
    /// A connect attempt is in flight.
    Connecting,
    /// `ensure_daemon` is spawning `txtodod`.
    Spawning,
    /// Every retry failed; the frontend shows the reconnect banner with its retry button.
    Dead,
}
