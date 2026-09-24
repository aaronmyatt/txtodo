//! `txtodo pair [CODE]` (plan M4, design §4): the initiator (`txtodo pair`) starts an X25519
//! handshake on this device's own daemon and shows a QR/text code; the joiner (`txtodo pair
//! <code>`) decodes it and shows the six-word SAS to compare by eye
//! (`tasks/sync-pairing/notes.md`). Needs the daemon, like `log`/`blame`/`undo` — pairing lives
//! entirely in `PairOffer`/`PairAccept`/`PairConfirmSas`/`PairAwaitPeer`
//! (`crates/txtodo-daemon/src/pairing_grpc.rs`).
//!
//! Both sides must explicitly confirm before the group key ever moves: `tasks/sync-pairing/
//! notes.md`'s "Confirmation must be mutual" rule. A "no" or a garbled compare aborts without
//! ever calling `PairConfirmSas` — a false confirmation is worse than a failed pairing.
//!
//! **The cross-device leg is real now** (plan M4 `sync-pairing`'s LAN wiring pass,
//! `crates/txtodo-daemon/src/pairing_lan.rs`): the joiner's daemon finds the initiator over the
//! real LAN transport and carries this device's key and confirmation to it in the background, so
//! `run_offer` polls `PairAwaitPeer` for the real SAS once a joiner connects, and `run_join` polls
//! `Health.lan_group_key_present` for the real group key once the initiator also confirms. Both
//! polls are bounded (see `AWAIT_PEER_TIMEOUT`) rather than hanging forever if the other device
//! never shows up.

use crate::client::Daemon;
use crate::config::IdentityMode;
use crate::{CliError, Ctx};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::time::{Duration, Instant};

/// How long the CLI keeps polling `PairAwaitPeer`/`Health` before giving up — a little past the
/// daemon's own `PAIRING_WINDOW_MS` (120 000 ms, `txtodo_sync::PAIRING_WINDOW_MS`; this crate may
/// not depend on `txtodo-sync`, slice rule, so the value is duplicated here as a plain constant)
/// so the daemon's own window-expiry is what actually ends a stuck wait, not a shorter client one.
const AWAIT_PEER_TIMEOUT: Duration = Duration::from_millis(125_000);
/// Gap between polls — frequent enough to feel responsive, far below the poll's own RPC cost.
const AWAIT_PEER_POLL: Duration = Duration::from_millis(500);

/// The pairing offer's nine fields, field-for-field the same shape
/// `crates/txtodo-daemon/src/pairing_wire.rs`'s `RawCode` parses (`device`, `group_id`,
/// `x25519_pub`, `endpoint`, `nonce`, `identity_mode`, `relay_node_id`, `relay_url`,
/// `workspace_id`) — this crate may not depend on txtodo-daemon or txtodo-sync (slice rule: `May
/// depend only on: txtodo-core, txtodo-proto`), so this shape, built straight from
/// `pb::PairOfferResponse`'s own already-encoded string fields, is the only contract the two
/// crates share. Two independent wire encodings ride this one struct (task `pairing-code-compact`):
/// JSON for the QR ([`to_json`]/[`from_json`], unchanged since `pairing_wire.rs`'s own decode
/// still needs to recognize an existing scanner's output byte-for-byte) and postcard-packed,
/// base32-encoded text for the copy-paste fallback ([`to_compact`]/[`from_compact`]) — shorter,
/// and doesn't read like source code. [`parse_code`] tries either, the same way the daemon's own
/// `decode_wire` does. `relay_node_id`/`relay_url` (plan M8 `sync-pairing-relay`) are empty
/// strings, not absent, when the initiator has no relay configured — `#[serde(default)]` so a code
/// without these fields at all still decodes. `workspace_id` (task `pairing-workspace-identity`)
/// is the same way: this struct only needs to round-trip it into the outgoing code text (the
/// daemon reads it back out of the raw `code` string itself, not through this struct — see
/// `pairing_wire.rs::code_workspace_id`'s own doc).
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
    /// The initiator's relay node id, hex-encoded; empty when it has no relay configured/bound.
    #[serde(default)]
    relay_node_id: String,
    /// The relay URL `relay_node_id` is reachable through; empty exactly when it is.
    #[serde(default)]
    relay_url: String,
    /// The initiator's real, catalog-assigned `WorkspaceId` (ULID text) for the workspace being
    /// offered — the joiner's daemon adopts this verbatim so post-pairing sync routes correctly.
    #[serde(default)]
    workspace_id: String,
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
            relay_node_id: r.relay_node_id.clone(),
            relay_url: r.relay_url.clone(),
            workspace_id: r.workspace_id.clone(),
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

/// `txtodo pair`: starts the handshake, renders the QR and the text fallback, waits for a real
/// joiner to connect over the LAN transport (`PairAwaitPeer`), then shows and confirms the real
/// SAS. The joiner's own confirmation and the group key delivery happen in the background on both
/// daemons (`pairing_lan.rs`) — this command's own job ends once this device has confirmed.
fn run_offer(daemon: &mut Daemon) -> Result<(), CliError> {
    let offer = daemon.pair_offer()?;
    let code = PairingCode::from(&offer);
    print_qr(&to_json(&code)?)?;
    println!();
    println!("Code (no camera? paste this into `txtodo pair <code>` on the other device):");
    println!("{}", to_compact(&code)?);
    println!();
    println!("txtodo: waiting for a device to scan or enter this code...");
    let sas = await_peer_sas(daemon)?;
    println!();
    println!("Six words from the joining device — compare them by eye:");
    println!();
    println!("  {sas}");
    println!();
    if !confirm("Do the six words match exactly on both devices? [y/N] ")? {
        return Err(CliError::Message(
            "txtodo: pairing aborted — the words were not confirmed as matching. A mismatch can \
             mean an active attacker; do not retry blindly, start a fresh pairing instead."
                .to_owned(),
        ));
    }
    daemon.pair_confirm_sas(ask_own_device()?)?;
    println!(
        "Confirmed on this device. Once the joining device also confirms, it receives the group \
         key and syncs this workspace over the LAN automatically."
    );
    Ok(())
}

/// Polls `PairAwaitPeer` (empty `sas` means "still waiting") until a joiner's handshake reaches
/// this device, or [`AWAIT_PEER_TIMEOUT`] passes.
fn await_peer_sas(daemon: &mut Daemon) -> Result<String, CliError> {
    let start = Instant::now();
    loop {
        let result = daemon.pair_await_peer()?;
        if !result.sas.is_empty() {
            return Ok(result.sas);
        }
        if start.elapsed() >= AWAIT_PEER_TIMEOUT {
            return Err(CliError::Message(
                "txtodo: no device joined this pairing within the window. Run `txtodo pair` \
                 again for a fresh code."
                    .to_owned(),
            ));
        }
        std::thread::sleep(AWAIT_PEER_POLL);
    }
}

/// `txtodo pair <code>`: decodes the offer, refuses a detected `identity_mode` mismatch (Q6),
/// shows the real SAS, requires an explicit match confirmation, confirms, then waits for the real
/// group key to land (the daemon's background relay carries this device's confirmation to the
/// initiator and the initiator's sealed grant back — `pairing_lan.rs`) before reporting a
/// snapshot of what actually synced.
fn run_join(ctx: &Ctx, daemon: &mut Daemon, code: &str) -> Result<(), CliError> {
    let parsed: PairingCode = parse_code(code)?;
    refuse_on_identity_mismatch(ctx, daemon, &parsed.identity_mode)?;
    let result = daemon.pair_accept(code.to_owned())?;
    if result.kept_own_workspace {
        println!(
            "Your default list stays as it is. The other device's workspace arrives separately, \
             as a Remote workspace (`txtodo workspace list`)."
        );
    }
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
    daemon.pair_confirm_sas(ask_own_device()?)?;
    println!("Confirmed on this device. Waiting for the initiator to confirm...");
    let carrier = await_group_key(daemon)?;
    println!("Paired. Syncing this workspace with the initiator over {carrier}...");
    wait_for_convergence(daemon)?;
    print_workspace_snapshot(daemon)?;
    Ok(())
}

/// Polls `Health.lan_group_key_present` until the real group key this device's background relay
/// task adopted (`Workspace::adopt_group_key`) shows up, or [`AWAIT_PEER_TIMEOUT`] passes. Returns
/// the carrier phrase for `run_join`'s own print, read off `Health.pairing_last_carrier`
/// (`pairing_lan_state.rs::PairingLan::record_carrier`, the same field `doctor_transport.rs`
/// already reports) rather than assuming LAN — pairing can just as well have completed over the
/// relay (`sync-pairing-relay`).
fn await_group_key(daemon: &mut Daemon) -> Result<&'static str, CliError> {
    let start = Instant::now();
    loop {
        let health = daemon.health()?;
        if health.lan_group_key_present {
            return Ok(carrier_phrase(&health.pairing_last_carrier));
        }
        if start.elapsed() >= AWAIT_PEER_TIMEOUT {
            // This side only ever sees "no group key landed": it cannot tell an unconfirmed
            // initiator from a reply that never made it back, so the message must not guess.
            return Err(CliError::Message(
                "txtodo: no reply from the initiator's device within the pairing window. Nothing \
                 was adopted on this device. Check both daemons' logs for `pairing_` lines \
                 (`txtodo doctor` shows where they are), then run `txtodo pair <code>` again with \
                 a fresh code."
                    .to_owned(),
            ));
        }
        std::thread::sleep(AWAIT_PEER_POLL);
    }
}

/// `carrier` is `Health.pairing_last_carrier`: `"lan"`, `"relay"`, or (should not happen here,
/// since we only call this once the group key has actually landed) empty.
fn carrier_phrase(carrier: &str) -> &'static str {
    match carrier {
        "lan" => "the LAN",
        "relay" => "the relay",
        _ => "the network",
    }
}

/// How long to give the LAN sync engine (`lan.rs`, unchanged by this task — its own periodic
/// redial is what actually pulls the initiator's ops once the group key and id match) a bounded
/// moment to converge before printing the snapshot. Real convergence measured sub-second in
/// `lan_loopback_converge.rs`; this is a courtesy wait, not a guarantee — `txtodo listfile` always
/// shows the latest state afterward regardless.
const CONVERGE_GRACE: Duration = Duration::from_secs(5);
const CONVERGE_POLL: Duration = Duration::from_millis(250);

fn wait_for_convergence(daemon: &mut Daemon) -> Result<(), CliError> {
    let start = Instant::now();
    loop {
        let files = daemon.list_files()?;
        let has_content = files
            .iter()
            .any(|f| f.progress.as_ref().is_some_and(|p| p.total > 0));
        if has_content || start.elapsed() >= CONVERGE_GRACE {
            return Ok(());
        }
        std::thread::sleep(CONVERGE_POLL);
    }
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
/// `ListFiles`/`GetFile` say this workspace holds right now. There is no dedicated
/// pairing-snapshot RPC — once the group id/key match, `lan.rs`'s existing group-keyed sync
/// engine is the real vehicle that delivers the initiator's files (a full op-log replay from
/// genesis, the same mechanism `nested_ref_sync.rs` proved for a fresh device), so this reuses
/// the two RPCs every other listing command already does rather than a bespoke transfer.
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

/// Task `default-workspace-pairing-consent` (decided 2026-09-24): the default list merges only
/// between two devices whose humans both answer yes; with any other device each side sees the
/// other's default as a separate Remote workspace. Same explicit-yes rule as the SAS question: no
/// answer, or anything but yes, is no.
fn ask_own_device() -> Result<bool, CliError> {
    println!();
    println!("Your default todo list merges only with devices that are yours.");
    confirm("Is the other device your own? [y/N] ")
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

/// The compact text fallback (task `pairing-code-compact`): postcard-packed, then base32 (RFC
/// 4648, no padding) — shorter than JSON and doesn't read like source code, for the common
/// desktop-to-desktop case where there's no camera to scan a QR with. Same codec
/// `crates/txtodo-daemon/src/pairing_wire.rs`'s `decode_wire` accepts on the other end.
fn to_compact(code: &PairingCode) -> Result<String, CliError> {
    let bytes = postcard::to_allocvec(code)
        .map_err(|e| CliError::Message(format!("txtodo: cannot encode the pairing code: {e}")))?;
    Ok(data_encoding::BASE32_NOPAD.encode(&bytes))
}

/// Decodes a compact base32 code back into its fields.
fn from_compact(text: &str) -> Result<PairingCode, CliError> {
    let bytes = data_encoding::BASE32_NOPAD
        .decode(text.trim().to_ascii_uppercase().as_bytes())
        .map_err(|_| CliError::Message("txtodo: pairing code is not valid".to_owned()))?;
    postcard::from_bytes(&bytes)
        .map_err(|_| CliError::Message("txtodo: pairing code is not valid".to_owned()))
}

fn from_json(text: &str) -> Result<PairingCode, CliError> {
    serde_json::from_str(text)
        .map_err(|_| CliError::Message("txtodo: pairing code is not valid".to_owned()))
}

/// Tries either wire format a `code` might be in — JSON (a scanned QR) or the compact base32 form
/// (typed or pasted from [`to_compact`]'s own output) — the same `{`-prefix test
/// `pairing_wire.rs::decode_wire` uses on the daemon side, since the two alphabets never overlap.
fn parse_code(text: &str) -> Result<PairingCode, CliError> {
    if text.trim().starts_with('{') {
        from_json(text)
    } else {
        from_compact(text)
    }
}

#[cfg(test)]
#[path = "pair_tests.rs"]
mod tests;
