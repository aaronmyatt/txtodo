//! `txtodo workspace offers|accept|decline` (task `workspace-offer-cli`): the CLI surface of the
//! offer/accept workspace-identity agreement (`tasks/daemon-workspace-identity-agreement`). A
//! paired peer announces its non-default workspaces; each lands here as a pending offer that this
//! device adopts (same `WorkspaceId`, a local directory of its choosing) or discards. Split out of
//! `workspace.rs` for its file-length budget only.
//!
//! An offer is keyed by (offering device, workspace id). The commands take the workspace id and
//! find the device among the pending offers, so the common case needs one id; `--from` picks the
//! device when two peers offer the same workspace.

use crate::client::Daemon;
use crate::{CliError, json};
use txtodo_proto::v1 as pb;

/// `workspace offers`: one row per pending offer, in the daemon's own order.
pub fn run_offers(daemon: &mut Daemon, as_json: bool) -> Result<(), CliError> {
    let offers = daemon.workspace_pending_offers()?;
    if offers.is_empty() {
        if !as_json {
            println!("TODO: no pending workspace offers.");
        }
        return Ok(());
    }
    for o in &offers {
        println!(
            "{}",
            if as_json {
                offer_json(o)
            } else {
                offer_text(o)
            }
        );
    }
    Ok(())
}

/// `workspace accept <id> --dir <path> [--from <device>]`: adopts the offer at `dir`. `--dir` is
/// required until `remote-workspace-mirror` decides a default location for a bare accept.
pub fn run_accept(
    daemon: &mut Daemon,
    id: &str,
    from: Option<&str>,
    dir: &str,
    as_json: bool,
) -> Result<(), CliError> {
    let device = resolve_device(daemon, id, from)?;
    let info = daemon.workspace_accept_offer(&device, id, dir)?;
    if as_json {
        println!(
            r#"{{"id":{},"root":{},"offering_device":{}}}"#,
            json::str(&info.workspace_id),
            json::str(&info.root),
            json::str(&device)
        );
    } else {
        println!(
            "TODO: workspace {} accepted at {}.",
            info.workspace_id, info.root
        );
    }
    Ok(())
}

/// `workspace decline <id> [--from <device>]`: discards the offer. What the offering device does
/// next (stop re-announcing) is the control channel's bookkeeping, not built yet.
pub fn run_decline(
    daemon: &mut Daemon,
    id: &str,
    from: Option<&str>,
    as_json: bool,
) -> Result<(), CliError> {
    let device = resolve_device(daemon, id, from)?;
    let declined = daemon.workspace_decline_offer(&device, id)?;
    if as_json {
        println!(r#"{{"declined":{declined}}}"#);
    } else if declined {
        println!("TODO: offer for workspace {id} from device {device} declined.");
    } else {
        return Err(CliError::Message(format!(
            "txtodo: no pending offer for workspace {id} from device {device}."
        )));
    }
    Ok(())
}

/// The offering device for `id`: `--from` verbatim when given, else the one pending offer that
/// names `id`. Zero or several matches are errors that name what to do.
fn resolve_device(daemon: &mut Daemon, id: &str, from: Option<&str>) -> Result<String, CliError> {
    if let Some(device) = from {
        return Ok(device.to_owned());
    }
    let offers = daemon.workspace_pending_offers()?;
    let devices: Vec<&str> = offers
        .iter()
        .filter(|o| o.workspace_id == id)
        .map(|o| o.offering_device.as_str())
        .collect();
    pick_device(id, &devices).map(str::to_owned)
}

/// Pure half of [`resolve_device`]: exactly one device, or an error naming the fix.
fn pick_device<'a>(id: &str, devices: &[&'a str]) -> Result<&'a str, CliError> {
    match devices {
        [one] => Ok(one),
        [] => Err(CliError::Message(format!(
            "txtodo: no pending offer for workspace {id}; see `txtodo workspace offers`."
        ))),
        many => Err(CliError::Message(format!(
            "txtodo: {} devices offer workspace {id}; pick one with --from ({}).",
            many.len(),
            many.join(", ")
        ))),
    }
}

fn offer_json(o: &pb::PendingWorkspaceOffer) -> String {
    format!(
        r#"{{"workspace_id":{},"offering_device":{},"name":{},"offered_at_ms":{}}}"#,
        json::str(&o.workspace_id),
        json::str(&o.offering_device),
        json::str(&o.name),
        o.offered_at_ms
    )
}

/// `<workspace id>  <name>  from <device>` — the id first, like `workspace list`, so a row's
/// first word is what `accept`/`decline` take.
fn offer_text(o: &pb::PendingWorkspaceOffer) -> String {
    let name = if o.name.is_empty() {
        "(unnamed)"
    } else {
        o.name.as_str()
    };
    format!("{}  {name}  from {}", o.workspace_id, o.offering_device)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offer(name: &str) -> pb::PendingWorkspaceOffer {
        pb::PendingWorkspaceOffer {
            offering_device: "01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
            workspace_id: "01BX5ZZKBKACTAV9WEVGEMMVRZ".into(),
            name: name.into(),
            offered_at_ms: 1_000,
        }
    }

    #[test]
    fn text_row_leads_with_the_workspace_id_and_names_the_device() {
        let row = offer_text(&offer("work"));
        assert!(row.starts_with("01BX5ZZKBKACTAV9WEVGEMMVRZ  work"), "{row}");
        assert!(row.ends_with("from 01ARZ3NDEKTSV4RRFFQ69G5FAV"), "{row}");
        assert!(offer_text(&offer("")).contains("(unnamed)"));
        assert!(offer_json(&offer("work")).contains(r#""name":"work""#));
    }

    #[test]
    fn pick_device_needs_exactly_one_match() {
        assert_eq!(pick_device("w", &["d1"]).ok(), Some("d1"));
        let none = pick_device("w", &[]).err().map(|e| e.to_string());
        assert!(
            none.as_deref()
                .is_some_and(|m| m.contains("no pending offer")),
            "{none:?}"
        );
        let many = pick_device("w", &["d1", "d2"]).err().map(|e| e.to_string());
        assert!(
            many.as_deref().is_some_and(|m| m.contains("--from")),
            "{many:?}"
        );
    }
}
