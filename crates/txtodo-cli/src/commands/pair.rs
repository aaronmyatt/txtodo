//! `txtodo pair [CODE]` (plan M4, design §4): the initiator (`txtodo pair`) starts an X25519
//! handshake on this device's own daemon and shows a QR/text code; the joiner (`txtodo pair
//! <code>`) decodes it and shows the six-word SAS to compare by eye
//! (`tasks/sync-pairing/notes.md`). Needs the daemon, like `log`/`blame`/`undo` — pairing lives
//! entirely in `PairOffer`/`PairAccept`/`PairConfirmSas`
//! (`crates/txtodo-daemon/src/pairing_grpc.rs`).
//!
//! Both sides must explicitly confirm before the group key ever moves: `tasks/sync-pairing/
//! notes.md`'s "Confirmation must be mutual" rule. A "no" or a garbled compare aborts without
//! ever calling `PairConfirmSas` — a false confirmation is worse than a failed pairing.
//!
//! **The cross-device leg is not built yet.** `PairOffer`/`PairAccept`/`PairConfirmSas` are calls
//! to *this device's own* daemon only (`pairing_grpc.rs`'s module doc): nothing yet carries the
//! joiner's public key back to the initiator, or the sealed group key back to the joiner — that
//! is `sync-lan-transport`, separate and not landed, and the proto messages themselves have no
//! field to carry either today. So `txtodo pair` (initiator) can show its offer but cannot yet
//! learn a peer's key or display a SAS; `txtodo pair <code>` (joiner) computes and confirms its
//! own SAS for real, but the group key and the snapshot it unlocks wait on the same missing leg.
//! Both paths say so plainly below rather than hang or fabricate progress.

use crate::client::Daemon;
use crate::config::IdentityMode;
use crate::{CliError, Ctx};
use serde::{Deserialize, Serialize};
use std::io::Write;

/// The JSON `code` a QR encodes and `txtodo pair <code>` accepts: field-for-field the same shape
/// `crates/txtodo-daemon/src/pairing_wire.rs` parses (`device`, `group_id`, `x25519_pub`,
/// `endpoint`, `nonce`, `identity_mode`) — this crate may not depend on txtodo-daemon or
/// txtodo-sync (slice rule: `May depend only on: txtodo-core, txtodo-proto`), so this JSON shape,
/// built straight from `pb::PairOfferResponse`'s own already-encoded string fields, is the only
/// contract the two crates share.
#[derive(Debug, Serialize, Deserialize)]
struct PairingCode {
    device: String,
    group_id: String,
    x25519_pub: String,
    endpoint: String,
    nonce: String,
    /// `"tagged"` or `"sidecar"` (docs/questions.md Q2), the initiator's own — carried so the
    /// joiner can detect a mismatch and refuse rather than guess a merge (Q6, open).
    identity_mode: String,
}

impl From<&txtodo_proto::v1::PairOfferResponse> for PairingCode {
    fn from(r: &txtodo_proto::v1::PairOfferResponse) -> PairingCode {
        PairingCode {
            device: r.device.clone(),
            group_id: r.group_id.clone(),
            x25519_pub: r.x25519_pub.clone(),
            endpoint: r.endpoint.clone(),
            nonce: r.nonce.clone(),
            identity_mode: r.identity_mode.clone(),
        }
    }
}

/// Entry point: no `code` is the initiator, a `code` is the joiner (notes.md's own command shape:
/// "the other device runs `txtodo pair <code>` or scans").
pub fn run(ctx: &Ctx, daemon: &mut Daemon, code: Option<&str>) -> Result<(), CliError> {
    match code {
        None => run_offer(daemon),
        Some(code) => run_join(ctx, daemon, code),
    }
}

/// `txtodo pair`: starts the handshake, renders the QR and the text fallback, and explains the
/// current limit (see the module doc) instead of hanging.
fn run_offer(daemon: &mut Daemon) -> Result<(), CliError> {
    let offer = daemon.pair_offer()?;
    let code = PairingCode::from(&offer);
    let text = to_json(&code)?;
    print_qr(&text)?;
    println!();
    println!("Code (no camera? paste this into `txtodo pair <code>` on the other device):");
    println!("{text}");
    println!();
    println!(
        "txtodo: waiting for a device to scan or enter this code. This build cannot yet finish \
         the handshake or show this device's own six words automatically — the network transport \
         between two txtodo daemons has not landed (tracked separately), so nothing carries the \
         joining device's key back here. This is a known limitation, not a hang: the command has \
         nothing left to do until that transport exists."
    );
    Ok(())
}

/// `txtodo pair <code>`: decodes the offer, refuses a detected `identity_mode` mismatch (Q6),
/// shows the real SAS, requires an explicit match confirmation, then confirms and reports the
/// current workspace snapshot.
fn run_join(ctx: &Ctx, daemon: &mut Daemon, code: &str) -> Result<(), CliError> {
    let parsed: PairingCode = from_json(code)?;
    refuse_on_identity_mismatch(ctx, daemon, &parsed.identity_mode)?;
    let result = daemon.pair_accept(code.to_owned())?;
    println!("Six words from the initiator's device — compare them by eye:");
    println!();
    println!("  {}", result.sas);
    println!();
    if !confirm("Do the six words match exactly on both devices? [y/N] ")? {
        return Err(CliError::Message(
            "txtodo: pairing aborted — the words were not confirmed as matching. A mismatch can \
             mean an active attacker; do not retry blindly, start a fresh pairing instead."
                .to_owned(),
        ));
    }
    daemon.pair_confirm_sas()?;
    println!("Confirmed on this device.");
    print_workspace_snapshot(daemon)?;
    println!(
        "txtodo: still waiting on the initiator's own confirmation and the group key — the \
         daemon-to-daemon transport has not landed yet (see this command's own doc comment), so \
         that leg cannot complete today."
    );
    Ok(())
}

/// Refuses a detected `identity_mode` mismatch unless this workspace has no tasks yet (nothing to
/// desync) — the two happy paths this task implements. Never guesses which mode should win: that
/// policy decision is docs/questions.md Q6, still open.
fn refuse_on_identity_mismatch(
    ctx: &Ctx,
    daemon: &mut Daemon,
    initiator_mode: &str,
) -> Result<(), CliError> {
    let local = ctx.config.identity_mode();
    if identity_mode_matches(local, initiator_mode) {
        return Ok(());
    }
    if workspace_is_empty(daemon)? {
        return Ok(());
    }
    Err(CliError::Message(format!(
        "txtodo: refusing to pair — this device's identity_mode ({local:?}) does not match the \
         initiator's ({initiator_mode:?}), and this workspace already has tasks. Reconciling \
         differing identity_mode is an open question (docs/questions.md Q6); pairing will not \
         guess a merge. Re-run once Q6 is answered, or pair from an empty workspace."
    )))
}

/// Whether `local` is the same mode as the initiator's own wire string.
fn identity_mode_matches(local: IdentityMode, remote: &str) -> bool {
    matches!(
        (local, remote),
        (IdentityMode::Tagged, "tagged") | (IdentityMode::Sidecar, "sidecar")
    )
}

/// No task lines anywhere in this workspace yet (every `FileInfo.progress.total` is zero) — the
/// Q6-safe case where adopting a mismatched mode has nothing on disk to desync.
fn workspace_is_empty(daemon: &mut Daemon) -> Result<bool, CliError> {
    let files = daemon.list_files()?;
    Ok(files
        .iter()
        .all(|f| f.progress.as_ref().is_none_or(|p| p.total == 0)))
}

/// The "then a snapshot" half of "snapshot then ops" (`tasks/sync-pairing/notes.md`): what
/// `ListFiles`/`GetFile` say this workspace holds right now, the same pull a freshly paired
/// device performs to catch up. There is no dedicated pairing-snapshot RPC (checked
/// `pairing_grpc.rs`), so this reuses the two RPCs every other listing command already does.
fn print_workspace_snapshot(daemon: &mut Daemon) -> Result<(), CliError> {
    let files = daemon.list_files()?;
    println!("Workspace snapshot ({} file(s)):", files.len());
    for f in &files {
        let bytes = daemon.get(&f.path)?;
        println!("  {} ({} bytes)", f.path, bytes.len());
    }
    Ok(())
}

/// Renders `text` as a terminal QR code (half-block glyphs, two QR rows per printed line).
fn print_qr(text: &str) -> Result<(), CliError> {
    let code = qrcode::QrCode::new(text.as_bytes())
        .map_err(|e| CliError::Message(format!("txtodo: cannot render a QR code: {e}")))?;
    let art = code.render::<qrcode::render::unicode::Dense1x2>().build();
    println!("{art}");
    Ok(())
}

/// Reads one line from stdin and requires an explicit "y"/"yes" (case-insensitive); anything
/// else, including EOF, is no. Never defaults to yes
/// (`tasks/sync-pairing/notes.md`'s "Confirmation must be mutual").
fn confirm(prompt: &str) -> Result<bool, CliError> {
    print!("{prompt}");
    std::io::stdout().flush().map_err(CliError::Io)?;
    let mut line = String::new();
    let n = std::io::stdin()
        .read_line(&mut line)
        .map_err(CliError::Io)?;
    Ok(n > 0 && is_explicit_yes(&line))
}

/// The pure "is this an explicit yes" rule [`confirm`] applies to one line of input.
fn is_explicit_yes(line: &str) -> bool {
    let answer = line.trim().to_ascii_lowercase();
    answer == "y" || answer == "yes"
}

fn to_json(code: &PairingCode) -> Result<String, CliError> {
    serde_json::to_string(code)
        .map_err(|e| CliError::Message(format!("txtodo: cannot encode the pairing code: {e}")))
}

fn from_json(text: &str) -> Result<PairingCode, CliError> {
    serde_json::from_str(text)
        .map_err(|_| CliError::Message("txtodo: pairing code is not valid".to_owned()))
}

#[cfg(test)]
#[path = "pair_tests.rs"]
mod tests;
