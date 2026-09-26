//! `txtodo doctor`'s `sync` rows (task sync-drift line 7), from `SyncStatus`: per peer, each
//! workspace where its incoming ops keep being refused, and whether the daemon has parked it for
//! holding no key we share (line 5). The daemon keeps both in memory, so a restart clears them
//! until the next refusal. An older daemon sends neither, and with no daemon there is no status:
//! no rows either way.

use super::doctor::{Check, Status, check};
use std::path::Path;
use txtodo_proto::v1 as pb;

/// Refusals in a row from which a stuck file is a FAIL, not a warn: one can be a race the resend
/// fixes a few seconds later; two means the resend was refused too.
pub(super) const STUCK_FAIL_AFTER: u32 = 2;

/// One row per stuck file, then one per parked peer, in the daemon's peer order.
pub(super) fn sync_checks(
    status: Option<&pb::SyncStatusResponse>,
    devices: &[pb::Device],
    workspaces: &[pb::WorkspaceInfo],
) -> Vec<Check> {
    let Some(status) = status else {
        return Vec::new();
    };
    let mut rows = Vec::new();
    for peer in &status.peers {
        let label = peer_label(&peer.device, devices);
        rows.extend(peer.stuck.iter().map(|s| stuck_row(&label, s, workspaces)));
        if peer.parked {
            rows.push(parked_row(&label));
        }
    }
    rows
}

/// `name (id)` when the device list names the peer, else its id.
fn peer_label(id: &str, devices: &[pb::Device]) -> String {
    match devices.iter().find(|d| d.id == id) {
        Some(d) if !d.name.is_empty() => format!("{} ({id})", d.name),
        _ => id.to_owned(),
    }
}

fn stuck_row(
    label: &str,
    s: &pb::sync_status_response::Stuck,
    workspaces: &[pb::WorkspaceInfo],
) -> Check {
    let status = if s.refusals >= STUCK_FAIL_AFTER {
        Status::Fail
    } else {
        Status::Warn
    };
    let plural = if s.refusals == 1 { "" } else { "s" };
    check(
        "sync",
        status,
        format!(
            "{label}: stuck on {} since {} ({} refusal{plural} in a row): {}; its later ops wait \
             behind it",
            stuck_where(s, workspaces),
            local_time(s.since_ms),
            s.refusals,
            s.reason
        ),
    )
}

/// The file's full path when its workspace is registered here, else the file and workspace id.
fn stuck_where(s: &pb::sync_status_response::Stuck, workspaces: &[pb::WorkspaceInfo]) -> String {
    match workspaces.iter().find(|w| w.workspace_id == s.workspace_id) {
        Some(w) => Path::new(&w.root).join(&s.file).display().to_string(),
        None => format!("{} in workspace {}", s.file, s.workspace_id),
    }
}

/// Unix ms as local `YYYY-MM-DD HH:MM:SS`; the raw number when jiff refuses it.
/// Ref: https://docs.rs/jiff/latest/jiff/struct.Timestamp.html#method.from_millisecond
fn local_time(ms: u64) -> String {
    i64::try_from(ms)
        .ok()
        .and_then(|ms| jiff::Timestamp::from_millisecond(ms).ok())
        .map_or_else(
            || ms.to_string(),
            |t| {
                t.to_zoned(jiff::tz::TimeZone::system())
                    .strftime("%Y-%m-%d %H:%M:%S")
                    .to_string()
            },
        )
}

fn parked_row(label: &str) -> Check {
    check(
        "sync",
        Status::Warn,
        format!(
            "{label}: parked, its frames do not open under our group key; not dialed until it \
             pairs again, shows up on the LAN in our group, or the daemon restarts"
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const PEER: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
    const WS: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAW";

    fn stuck(refusals: u32) -> pb::sync_status_response::Stuck {
        pb::sync_status_response::Stuck {
            workspace_id: WS.into(),
            file: "tasks/a/todo.txt".into(),
            reason: "store: UNIQUE constraint failed: ops.op_id".into(),
            since_ms: 1_790_000_000_000,
            last_ms: 1_790_000_030_000,
            refusals,
        }
    }

    fn status(stuck: Vec<pb::sync_status_response::Stuck>, parked: bool) -> pb::SyncStatusResponse {
        pb::SyncStatusResponse {
            peers: vec![pb::sync_status_response::Peer {
                device: PEER.into(),
                stuck,
                parked,
                ..pb::sync_status_response::Peer::default()
            }],
            pending_ops: 0,
        }
    }

    fn named_peer() -> pb::Device {
        pb::Device {
            id: PEER.into(),
            name: "laptop".into(),
            ..pb::Device::default()
        }
    }

    #[test]
    fn a_stuck_file_names_its_path_reason_and_count_and_fails_once_resent() {
        let root = pb::WorkspaceInfo {
            workspace_id: WS.into(),
            root: "/home/me/todo".into(),
            ..pb::WorkspaceInfo::default()
        };
        let rows = sync_checks(
            Some(&status(vec![stuck(4)], false)),
            &[named_peer()],
            &[root],
        );
        assert_eq!(rows.len(), 1);
        assert_eq!((rows[0].name, rows[0].status), ("sync", Status::Fail));
        let detail = &rows[0].detail;
        assert!(
            detail.starts_with(&format!("laptop ({PEER}): stuck on ")),
            "{detail}"
        );
        // Built the way the row builds it: Windows CI joins with `\`.
        let path = Path::new("/home/me/todo").join("tasks/a/todo.txt");
        assert!(
            detail.contains(&format!("{} since ", path.display())),
            "{detail}"
        );
        assert!(
            detail.contains("(4 refusals in a row): store: UNIQUE"),
            "{detail}"
        );
    }

    #[test]
    fn one_refusal_is_a_warn_and_an_unknown_workspace_is_named_by_id() {
        let rows = sync_checks(Some(&status(vec![stuck(1)], false)), &[], &[]);
        assert_eq!(rows[0].status, Status::Warn);
        let detail = &rows[0].detail;
        assert!(
            detail.starts_with(&format!("{PEER}: stuck on ")),
            "{detail}"
        );
        assert!(
            detail.contains(&format!("tasks/a/todo.txt in workspace {WS}")),
            "{detail}"
        );
        assert!(detail.contains("(1 refusal in a row)"), "{detail}");
    }

    #[test]
    fn a_parked_peer_is_a_warn_and_nothing_known_is_no_row() {
        let rows = sync_checks(Some(&status(Vec::new(), true)), &[named_peer()], &[]);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].status, Status::Warn);
        assert!(rows[0].detail.contains("laptop"), "{rows:?}");
        assert!(rows[0].detail.contains("parked"), "{rows:?}");
        assert!(sync_checks(Some(&status(Vec::new(), false)), &[], &[]).is_empty());
        assert!(sync_checks(None, &[], &[]).is_empty(), "no daemon, no rows");
    }

    #[test]
    fn a_time_jiff_refuses_prints_as_the_raw_number() {
        assert_eq!(local_time(u64::MAX), u64::MAX.to_string());
        assert_eq!(
            local_time(1_790_000_000_000).len(),
            19,
            "YYYY-MM-DD HH:MM:SS"
        );
    }
}
