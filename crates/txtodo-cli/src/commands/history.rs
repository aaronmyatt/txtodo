//! `txtodo log`, `blame <line>`, `undo`, `checkout <iso-datetime>` (plan M3, design §4.8). All
//! four need the daemon: history is the op log, and undo is ops that (later) sync. In direct-file
//! mode they fail with the fix.

use crate::client::Daemon;
use crate::{CliError, json};
use std::io::Write;
use txtodo_core::{LineKind, parse_file};
use txtodo_proto::v1 as pb;

/// `log` default page.
pub const LOG_DEFAULT_LIMIT: u32 = 50;
/// The message for the direct-file mode failure.
pub const NEEDS_DAEMON: &str = "needs the daemon: run `txtodo daemon start`, or `txtodo doctor`";

fn hlc_text(op: &pb::OpSummary) -> String {
    // Unix ms → local ISO seconds; the counter disambiguates same-millisecond ops.
    let secs = i64::try_from(op.hlc_wall_ms / 1000).unwrap_or(0);
    let stamp = jiff::Timestamp::from_second(secs)
        .map(|t| {
            t.to_zoned(jiff::tz::TimeZone::system())
                .strftime("%Y-%m-%dT%H:%M:%S")
                .to_string()
        })
        .unwrap_or_else(|_| op.hlc_wall_ms.to_string());
    format!("{stamp}.{:03}+{}", op.hlc_wall_ms % 1000, op.hlc_counter)
}

fn op_json(op: &pb::OpSummary) -> String {
    format!(
        r#"{{"seq":{},"hlc":{},"principal":{},"kind":{},"task":{},"summary":{}}}"#,
        op.seq,
        json::str(&hlc_text(op)),
        json::str(&op.principal),
        json::str(&op.kind),
        json::str(&op.task_id),
        json::str(&op.summary)
    )
}

fn print_ops(ops: &[pb::OpSummary], as_json: bool) {
    let out = std::io::stdout();
    let mut w = out.lock();
    for op in ops {
        let line = if as_json {
            op_json(op)
        } else {
            format!(
                "{:>6}  {}  {:<32}  {:<12}  {}",
                op.seq,
                hlc_text(op),
                op.principal,
                op.kind,
                op.summary
            )
        };
        // stdout closing early (a pipe to head) is not an error worth reporting.
        let _ = writeln!(w, "{line}");
    }
}

/// `log [--file F] [-n N]`: newest first.
pub fn run_log(
    daemon: &mut Daemon,
    file: Option<&str>,
    limit: Option<u32>,
    as_json: bool,
) -> Result<(), CliError> {
    let req = pb::HistoryRequest {
        path: file.unwrap_or_default().to_owned(),
        task_id: String::new(),
        limit: limit.unwrap_or(LOG_DEFAULT_LIMIT),
        before_seq: 0,
    };
    let ops = daemon.history(req)?;
    debug_assert!(ops.windows(2).all(|w| w[0].seq > w[1].seq), "newest first");
    print_ops(&ops, as_json);
    Ok(())
}

/// `blame <line>`: for the task at that line of todo.txt, the newest op per kind.
pub fn run_blame(daemon: &mut Daemon, item: &str, as_json: bool) -> Result<(), CliError> {
    let n: usize = item.parse().map_err(|_| CliError::Usage("blame ITEM#"))?;
    let bytes = daemon.get("todo.txt")?;
    let file = parse_file(&bytes);
    let line = n
        .checked_sub(1)
        .and_then(|i| file.lines.get(i))
        .ok_or_else(|| CliError::Message(format!("TODO: No task {n}.")))?;
    let task_id = match line.parse().map(|l| l.kind) {
        Some(LineKind::Task(t)) => t.id().map(|u| u.to_string()),
        _ => None,
    };
    let Some(task_id) = task_id else {
        return Err(CliError::Message(format!("TODO: No task {n}.")));
    };
    let ops = daemon.history(pb::HistoryRequest {
        path: "todo.txt".into(),
        task_id,
        limit: 1_000,
        before_seq: 0,
    })?;
    // Newest op per kind is the "who last touched this field" view.
    let mut seen: Vec<&str> = Vec::new();
    let latest: Vec<pb::OpSummary> = ops
        .into_iter()
        .filter(|op| {
            let key = op.kind.clone();
            if seen.iter().any(|k| *k == key) {
                return false;
            }
            seen.push(Box::leak(key.into_boxed_str()));
            true
        })
        .collect();
    debug_assert!(latest.len() <= 7, "at most one row per op kind");
    print_ops(&latest, as_json);
    Ok(())
}

/// `undo [--steps N]`.
pub fn run_undo(daemon: &mut Daemon, steps: u32, as_json: bool) -> Result<(), CliError> {
    let rep = daemon.undo("todo.txt", steps.max(1))?;
    if as_json {
        println!(
            r#"{{"applied":{},"hash":{}}}"#,
            rep.applied,
            json::str(&hex(&rep.hash))
        );
    } else {
        println!("TODO: undid {} op(s).", rep.applied);
    }
    Ok(())
}

/// `checkout <iso-datetime> [--stdout] [--file F]`.
pub fn run_checkout(
    daemon: &mut Daemon,
    at: &str,
    file: &str,
    to_stdout: bool,
) -> Result<(), CliError> {
    let at_wall_ms = parse_local_datetime_ms(at).ok_or(CliError::Usage(
        "checkout YYYY-MM-DDTHH:MM[:SS] [--stdout] [--file F]",
    ))?;
    let bytes = daemon.checkout(file, at_wall_ms)?;
    if to_stdout {
        let _ = std::io::stdout().lock().write_all(&bytes);
        return Ok(());
    }
    let dir = std::env::temp_dir().join(format!("txtodo-checkout-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(CliError::Io)?;
    let path = dir.join(file.rsplit('/').next().unwrap_or(file));
    std::fs::write(&path, &bytes).map_err(CliError::Io)?;
    println!("{}", path.display());
    Ok(())
}

/// `YYYY-MM-DDTHH:MM[:SS]` in the local zone (ADR 0011) → Unix ms, inclusive of the second.
pub fn parse_local_datetime_ms(text: &str) -> Option<u64> {
    let dt: jiff::civil::DateTime = text.parse().ok()?;
    let zoned = dt.to_zoned(jiff::tz::TimeZone::system()).ok()?;
    let ms = zoned.timestamp().as_millisecond();
    debug_assert!(ms > 0, "dates before 1970 are not todo dates");
    u64::try_from(ms).ok().map(|m| m + 999)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_datetime_parses_and_is_inclusive_of_the_second() {
        let a = parse_local_datetime_ms("2026-09-11T09:00").unwrap();
        let b = parse_local_datetime_ms("2026-09-11T09:00:00").unwrap();
        assert_eq!(a, b);
        assert_eq!(a % 1000, 999, "the whole second is included");
        assert!(parse_local_datetime_ms("yesterday").is_none());
        assert_eq!(hex(&[0, 255]), "00ff");
    }
}
