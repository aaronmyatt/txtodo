//! `doctor`'s build check (task version-info): is the daemon this CLI talks to the same build?
//! Split out of `doctor.rs` for its file budget, like `doctor_transport.rs`. The trigger,
//! 2026-09-20: an installed app spawned a bundled `txtodod` from before early bind, the CLI talked
//! to it, and nothing said it was an older build.

use super::doctor::{Check, Status, check};
use crate::buildinfo::{RELEASE_DATE, VERSION};
use txtodo_proto::v1 as pb;

/// Ok when the daemon reports this CLI's version and release date; a warning that names both and
/// the fix when either differs. A daemon that sends no release date is older than the field, so it
/// warns too. Never a failure: another build usually still works, and `doctor` exits 1 on a fail.
pub(super) fn version_check(health: Option<&pb::HealthResponse>) -> Check {
    let Some(h) = health else {
        return check(
            "version",
            Status::Ok,
            format!("{VERSION} ({RELEASE_DATE}); no daemon"),
        );
    };
    if h.version == VERSION && h.release_date == RELEASE_DATE {
        return check(
            "version",
            Status::Ok,
            format!("{VERSION} ({RELEASE_DATE}), the daemon too"),
        );
    }
    let daemon_date = if h.release_date.is_empty() {
        "no release date: an older build"
    } else {
        h.release_date.as_str()
    };
    check(
        "version",
        Status::Warn,
        format!(
            "this txtodo is {VERSION} ({RELEASE_DATE}), the daemon is {} ({daemon_date}); fix: \
             `txtodo daemon install --force` then `txtodo daemon start`, or reinstall the app \
             that started it",
            h.version
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn daemon(version: &str, release_date: &str) -> pb::HealthResponse {
        pb::HealthResponse {
            version: version.to_owned(),
            release_date: release_date.to_owned(),
            ..pb::HealthResponse::default()
        }
    }

    #[test]
    fn the_same_build_is_ok() {
        let c = version_check(Some(&daemon(VERSION, RELEASE_DATE)));
        assert_eq!(c.status, Status::Ok);
        assert!(c.detail.contains(VERSION), "{}", c.detail);
    }

    #[test]
    fn another_version_or_another_date_warns_and_names_both() {
        let older = version_check(Some(&daemon("0.0.1", "2026-09-17")));
        assert_eq!(older.status, Status::Warn);
        assert!(
            older.detail.contains("0.0.1 (2026-09-17)"),
            "{}",
            older.detail
        );
        assert!(older.detail.contains(VERSION) && older.detail.contains("daemon install"));
        let other_day = version_check(Some(&daemon(VERSION, "1999-01-01")));
        assert_eq!(other_day.status, Status::Warn);
    }

    #[test]
    fn a_daemon_with_no_release_date_is_an_older_build() {
        let c = version_check(Some(&daemon(VERSION, "")));
        assert_eq!(c.status, Status::Warn);
        assert!(c.detail.contains("older build"), "{}", c.detail);
    }

    #[test]
    fn no_daemon_is_not_a_warning() {
        assert_eq!(version_check(None).status, Status::Ok);
    }
}
