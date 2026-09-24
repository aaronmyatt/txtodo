//! One workspace's incoming `Ops` batch (task `sync-ack-before-held`, 2026-09-25): commit the
//! part that follows our heads and ack exactly that, skip a batch out of step with them (telling
//! the sender what we already hold), end the connection on anything else. Split out of
//! `lan_session_shared.rs` for its line budget; that file dispatches here.

use txtodo_model::Op;
use txtodo_store::WorkspaceId;
use txtodo_sync::{Link, Message, OriginRange, Session, SessionError};

use crate::lan_apply::{commit_incoming_ops, landed_ranges};
use crate::lan_session_shared::{SessionCtx, SyncError};

/// Why an `Ops` batch was not taken.
enum Refused {
    /// It does not follow the heads we hold (a `Gap`): a batch after one that failed, or a
    /// duplicate of one that already landed. Skipped; the sender re-serves from what we acked.
    /// Carries the `Ack` telling the sender what we already hold, when we hold part of it.
    OutOfStep(Option<Message>),
    /// Anything else (a bad signature, a message out of order): ends the connection.
    Fatal,
}

fn ops_or_refuse(
    ctx: &SessionCtx<'_>,
    session: &mut Session,
    msg: &Message,
) -> Result<Vec<Op>, Refused> {
    // An empty map when `msg` is not actually `Ops` is fine: `on_ops` checks the variant before
    // it ever consults `device_keys` and reports "unexpected message" instead.
    let device_keys = match msg {
        Message::Ops { ops, .. } => {
            crate::lan_apply::device_keys_for(ops, ctx.signing_key.public_key())
        }
        _ => std::collections::BTreeMap::new(),
    };
    match session.on_ops(ctx.workspace, msg, &device_keys) {
        Ok(ops) => Ok(ops),
        Err(SessionError::Gap(gap)) => {
            log_out_of_step(ctx.workspace, &gap);
            Err(Refused::OutOfStep(holding_ack(ctx, session, msg)))
        }
        Err(e) => {
            log_ops_refused(ctx.workspace, &e);
            Err(Refused::Fatal)
        }
    }
}

fn log_out_of_step(workspace: WorkspaceId, gap: &txtodo_sync::Gap) {
    tracing::debug!(%gap, %workspace, "lan_ops_out_of_step_skipped");
}

fn log_ops_refused(workspace: WorkspaceId, e: &SessionError) {
    tracing::warn!(error = %e, %workspace, "lan_ops_refused");
}

/// For a batch we already hold at least part of (a copy the sender sent again because our `Ack`
/// was still on its way), an `Ack` naming each such device's whole run up to our head: the sender
/// only takes each run's end as the peer's head (`lan_session_live.rs`), so it stops resending.
/// `None` when we hold none of it (we are behind: the sender's resend fills the hole).
fn holding_ack(ctx: &SessionCtx<'_>, session: &Session, msg: &Message) -> Option<Message> {
    let (Message::Ops { ranges, .. }, Ok(heads)) = (msg, session.heads(ctx.workspace)) else {
        return None;
    };
    let mut held: Vec<OriginRange> = Vec::new();
    for r in ranges {
        let head = heads.get(&r.device).copied().unwrap_or(0);
        if head >= r.first && !held.iter().any(|h| h.device == r.device) {
            held.push(OriginRange {
                device: r.device,
                first: 1,
                last: head,
            });
        }
    }
    (!held.is_empty()).then(|| Message::Ack {
        workspace: ctx.workspace.ulid().to_u128(),
        committed: held,
    })
}

/// Commits what it can of the batch and acks exactly that prefix (task `sync-ack-before-held`):
/// the heads advance only over ops that landed, and the sender re-serves the rest.
fn commit_and_ack(
    ctx: &SessionCtx<'_>,
    session: &mut Session,
    ops: Vec<Op>,
    ranges: Vec<OriginRange>,
) -> Option<Message> {
    let landed = commit_incoming_ops(ctx.ws, ctx.rt, ops);
    let committed_ranges = landed_ranges(&ranges, landed);
    match session.committed(ctx.workspace, &committed_ranges) {
        Ok(ack) => Some(ack),
        Err(e) => {
            tracing::warn!(error = %e, workspace = %ctx.workspace, "lan_ack_refused");
            None
        }
    }
}

/// `Ops` for `ctx.workspace`: commit what follows our heads and ack it, skip what does not.
/// `false` ends the connection.
pub(crate) fn handle_ops(
    link: &mut dyn Link,
    ctx: &SessionCtx<'_>,
    session: &mut Session,
    msg: &Message,
    ranges: Vec<OriginRange>,
) -> bool {
    // A batch out of step with our heads is skipped and the connection carries on (task
    // sync-ack-before-held): the sender sees no ack for it and re-serves from what we acked. Any
    // other refusal still ends the connection, and the reconnect's `Greet`/`Want` resyncs.
    let ops = match ops_or_refuse(ctx, session, msg) {
        Ok(ops) => ops,
        Err(Refused::OutOfStep(None)) => return true,
        Err(Refused::OutOfStep(Some(ack))) => {
            return ctx
                .send(link, ack)
                .map_or_else(|e| log_ack_send_failed(ctx.workspace, &e), |()| true);
        }
        Err(Refused::Fatal) => return false,
    };
    let Some(ack) = commit_and_ack(ctx, session, ops, ranges) else {
        return true;
    };
    match ctx.send(link, ack) {
        Ok(()) => true,
        Err(e) => log_ack_send_failed(ctx.workspace, &e),
    }
}

fn log_ack_send_failed(workspace: WorkspaceId, e: &SyncError) -> bool {
    tracing::warn!(error = %e, %workspace, "lan_ack_send_failed");
    false
}
