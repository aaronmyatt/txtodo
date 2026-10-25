//! Task `two-kinds-of-workspace` (ADR 0043): two real `txtodod` in global mode, one `txtodo pair`
//! between them, and the list one device made reaches the other with no accept and no command.
//! This replaces the old converge test, where the joiner's folder adopted the initiator's
//! workspace id: a list keeps its own id and a plain folder no longer syncs.
//!
//! The CLI's part is the whole path: `lists new` and `--list` make and fill the list through the
//! global daemon, `pair` runs the ceremony from the default workspace, and the file on B's disk is
//! the proof.

use crate::pairing::{assert_confirmed, finish_pair_offer, read_until_code};
use crate::support::global_daemon::GlobalDaemon;

use std::io::Write;
use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};

/// Generous: mDNS discovery, then a control session for the offer on the next redial.
const LIST_ARRIVES_WITHIN: Duration = Duration::from_secs(90);

/// Both humans answer "yes" (the Six words match) and "yes" (my own device).
const YES_YES: &str = "yes\nyes\n";

/// One device: a global daemon on its own state dir, and a folder to run `txtodo` from.
struct Device {
    daemon: GlobalDaemon,
    state: tempfile::TempDir,
    cwd: tempfile::TempDir,
}

fn device() -> Device {
    let tmp = || tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let (state, cwd) = (tmp(), tmp());
    // Production redials every 15 s; the deadline here assumes 1 s.
    let env = [("TXTODO_RESYNC_INTERVAL_MS", "1000")];
    let daemon = GlobalDaemon::spawn_with(state.path(), &env);
    Device { daemon, state, cwd }
}

impl Device {
    fn txtodo(&self, args: &[&str]) -> std::process::Output {
        self.daemon.txtodo(self.cwd.path(), args)
    }

    /// `txtodo pair CODE` as the joiner, with both answers fed at once.
    fn join(&self, code: &str) -> std::process::Output {
        let mut cmd = self.daemon.command(self.cwd.path());
        let mut child = cmd
            .args(["pair", code])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap_or_else(|e| panic!("spawn txtodo pair CODE: {e}"));
        feed(&mut child, YES_YES);
        child
            .wait_with_output()
            .unwrap_or_else(|e| panic!("wait: {e}"))
    }
}

/// Writes `text` to the child's stdin and closes it.
fn feed(child: &mut std::process::Child, text: &str) {
    let mut stdin = child
        .stdin
        .take()
        .unwrap_or_else(|| panic!("no piped stdin"));
    stdin
        .write_all(text.as_bytes())
        .unwrap_or_else(|e| panic!("write stdin: {e}"));
}

/// Waits until `path` holds `needle`; the text it last held is the failure message.
fn wait_for_text(path: &Path, needle: &str) {
    let start = Instant::now();
    loop {
        let text = std::fs::read_to_string(path).unwrap_or_default();
        if text.contains(needle) {
            return;
        }
        assert!(
            start.elapsed() < LIST_ARRIVES_WITHIN,
            "{} never held {needle:?} within {LIST_ARRIVES_WITHIN:?}; last: {text:?}",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(250));
    }
}

fn ok(out: &std::process::Output) -> String {
    let (stdout, stderr) = (&out.stdout, &out.stderr);
    let (stdout, stderr) = (
        String::from_utf8_lossy(stdout),
        String::from_utf8_lossy(stderr),
    );
    assert!(out.status.success(), "stdout: {stdout}\nstderr: {stderr}");
    stdout.into_owned()
}

#[test]
fn a_paired_device_receives_the_others_list_with_no_command() {
    let (a, b) = (device(), device());
    ok(&a.txtodo(&["lists", "new", "work"]));
    ok(&a.txtodo(&["--list", "work", "add", "buy milk"]));

    // A offers from its default workspace and B joins; both say it is their own device.
    let mut offer: std::process::Child = a
        .daemon
        .command(a.cwd.path())
        .arg("pair")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap_or_else(|e| panic!("spawn txtodo pair: {e}"));
    feed(&mut offer, YES_YES);
    let stdout = offer.stdout.take().expect("piped stdout");
    let mut reader = std::io::BufReader::new(stdout);
    let (_seen, code) = read_until_code(&mut reader);
    let joined = ok(&b.join(&code));
    assert_confirmed(&joined);
    let (offer_ok, rest) = finish_pair_offer(offer, reader);
    assert!(offer_ok, "the initiator's pair failed: {rest}");

    // B was never told about `work`: it arrives as a list of B's own, in B's lists folder.
    wait_for_text(&b.state.path().join("lists/work/todo.txt"), "buy milk");
    let listed = ok(&b.txtodo(&["lists"]));
    assert!(listed.contains("work"), "{listed}");
}
