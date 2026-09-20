//! Reading what the service manager prints about a unit: pure, so the tests need no launchd and no
//! systemd. Split out of `service.rs` for its line budget.

/// `systemctl is-active` prints one word; these two mean the unit is up or on its way up.
/// https://www.freedesktop.org/software/systemd/man/latest/systemctl.html#is-active%20PATTERN%E2%80%A6
pub(crate) fn systemd_state_is_loaded(stdout: &str) -> bool {
    matches!(stdout.trim(), "active" | "activating")
}

/// Whether `launchctl print <service-target>` describes a job that is up or about to be.
///
/// "Loaded" is not enough (code review 2026-09-20, finding 1). A `txtodod` that loses the pid lock
/// exits 0, and with `KeepAlive = { SuccessfulExit = false }` launchd leaves that job loaded and
/// never restarts it: `launchctl print` still succeeds, with `state = not running`. A client that
/// read "loaded" as "on its way up" waited out its whole timeout and never spawned a daemon.
///
/// The job's own state is the `state = ...` line indented by exactly one tab; deeper `state =`
/// lines belong to its endpoints. `spawn scheduled` is launchd about to start it.
/// https://keith.github.io/xcode-man-pages/launchctl.1.html
pub(crate) fn launchd_job_is_up(print_stdout: &str) -> bool {
    print_stdout
        .lines()
        .filter_map(|line| line.strip_prefix('\t'))
        .filter(|rest| !rest.starts_with('\t'))
        .find_map(|rest| rest.strip_prefix("state = "))
        .is_some_and(|state| matches!(state.trim(), "running" | "spawn scheduled"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape of real `launchctl print gui/501/com.txtodo.txtodod` output, trimmed.
    fn print_output(state: &str) -> String {
        format!(
            "gui/501/com.txtodo.txtodod = {{\n\tactive count = 1\n\tpath = /x.plist\n\t\
             state = {state}\n\n\tprogram = /x/txtodod\n\tendpoints = {{\n\t\t\"x\" = {{\n\t\t\t\
             state = active\n\t\t}}\n\t}}\n}}\n"
        )
    }

    #[test]
    fn a_running_or_scheduled_job_is_up() {
        assert!(launchd_job_is_up(&print_output("running")));
        assert!(launchd_job_is_up(&print_output("spawn scheduled")));
    }

    #[test]
    fn a_loaded_job_that_exited_is_not_up() {
        // What launchd prints after the daemon lost the pid lock and exited 0.
        assert!(!launchd_job_is_up(&print_output("not running")));
        assert!(!launchd_job_is_up(&print_output("exited")));
    }

    #[test]
    fn a_nested_endpoint_state_is_not_the_jobs_state() {
        let only_nested =
            "x = {\n\tendpoints = {\n\t\t\"x\" = {\n\t\t\tstate = running\n\t\t}\n\t}\n}\n";
        assert!(!launchd_job_is_up(only_nested));
        assert!(
            !launchd_job_is_up(""),
            "no output: launchctl could not be asked"
        );
    }
}
