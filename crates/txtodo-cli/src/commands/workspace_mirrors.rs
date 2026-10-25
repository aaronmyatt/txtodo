//! `txtodo workspace prune --repo-mirrors [--yes]` (task `two-kinds-of-workspace`, ADR 0043): moves
//! each Remote mirror an older build made aside (`MirrorsRetire`) and un-registers it. Dry by
//! default, like `prune --orphans`; the daemon never deletes, and the folder each mirror went to is
//! printed. `txtodo doctor` says which mirrors hold a line the same-named clone lacks.

use crate::client::Daemon;
use crate::{CliError, json};
use txtodo_proto::v1 as pb;

/// `workspace prune --repo-mirrors`: what would move (a dry run through the daemon), and with
/// `yes` what did.
pub fn run(daemon: &mut Daemon, yes: bool, as_json: bool) -> Result<(), CliError> {
    let mirrors = daemon.mirrors_retire("", !yes)?.mirrors;
    if as_json {
        mirrors.iter().for_each(|m| println!("{}", mirror_json(m)));
        return Ok(());
    }
    if mirrors.is_empty() {
        println!("TODO: no Remote mirrors.");
        return Ok(());
    }
    for m in &mirrors {
        println!("{}", mirror_text(m));
    }
    if yes {
        println!(
            "TODO: moved {} mirror(s) aside; nothing was deleted.",
            mirrors.len()
        );
    } else {
        println!(
            "TODO: run with --yes to move them aside (`txtodo doctor` shows what each holds)."
        );
    }
    Ok(())
}

fn mirror_text(m: &pb::RetiredMirror) -> String {
    let to = if m.moved_to.is_empty() {
        "its folder is gone".to_owned()
    } else {
        format!("to {}", m.moved_to)
    };
    format!("{}  {}  {}  {to}", m.workspace_id, m.name, m.root)
}

fn mirror_json(m: &pb::RetiredMirror) -> String {
    format!(
        r#"{{"workspace_id":{},"name":{},"root":{},"moved_to":{}}}"#,
        json::str(&m.workspace_id),
        json::str(&m.name),
        json::str(&m.root),
        json::str(&m.moved_to)
    )
}
