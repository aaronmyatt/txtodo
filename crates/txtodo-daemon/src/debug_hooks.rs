//! Test-only `Workspace` mutators for the LAN sync transport (plan M4 `sync-lan-transport`). Split
//! out of `workspace.rs` purely for the file budget, same pattern as `refdir.rs`/`notes_lookup.rs`
//! extending `ActorHandle` from their own files.
//!
//! Real pairing has no transport over the LAN link yet (`pairing_grpc.rs`'s own module doc), so a
//! real two-daemon test needs a scripted seam instead of a TTY SAS prompt. `debug_set_group_key`
//! is that seam: it is a plain, honest mutator with no guard of its own — the guard
//! (`TXTODO_TEST_HOOKS=1`) lives in `server.rs`'s gRPC handler, the only thing that ever calls it
//! outside a test, so the footgun is in exposing it over gRPC unguarded, which that caller does
//! not do.

use crate::server::TxtodoService;
use crate::workspace::Workspace;
use crate::workspace_error::WorkspaceError;
use crate::workspace_mint::GROUP_ID_KEY;
use tonic::{Request, Response, Status};
use txtodo_proto::v1 as pb;
use txtodo_sync::{GroupId, KeyId, Secret};

/// The env var that must be set to `1` for `debug_set_group_key` to do anything at all. Checked
/// here, at the gRPC boundary — `Workspace::debug_set_group_key` itself has no guard, so nothing
/// about this seam can leak into a path that does not deliberately check this first.
pub const TEST_HOOKS_ENV_VAR: &str = "TXTODO_TEST_HOOKS";

fn test_hooks_enabled() -> bool {
    std::env::var(TEST_HOOKS_ENV_VAR).as_deref() == Ok("1")
}

/// Lowercase (or uppercase) hex to bytes; `None` on anything else, never a panic on foreign input.
fn decode_hex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

impl TxtodoService {
    /// See the module doc and `TEST_HOOKS_ENV_VAR`: refused with `UNIMPLEMENTED` unless the
    /// daemon was started with `TXTODO_TEST_HOOKS=1`.
    pub(crate) async fn debug_set_group_key_impl(
        &self,
        r: Request<pb::DebugSetGroupKeyRequest>,
    ) -> Result<Response<pb::DebugSetGroupKeyResponse>, Status> {
        if !test_hooks_enabled() {
            return Err(Status::unimplemented(format!(
                "test hooks are disabled; set {TEST_HOOKS_ENV_VAR}=1 to enable this daemon-only \
                 debug seam"
            )));
        }
        let req = r.into_inner();
        let group = req
            .group_id
            .parse::<u128>()
            .map_err(|_| Status::invalid_argument("group_id must be a decimal u128"))?;
        let key_bytes = decode_hex(&req.key_hex)
            .ok_or_else(|| Status::invalid_argument("key_hex must be valid hex"))?;
        if key_bytes.len() != txtodo_sync::KEY_BYTES {
            return Err(Status::invalid_argument(format!(
                "key_hex must be exactly {} bytes, got {}",
                txtodo_sync::KEY_BYTES,
                key_bytes.len()
            )));
        }
        self.workspace()
            .debug_set_group_key(GroupId(group), key_bytes)
            .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(pb::DebugSetGroupKeyResponse {}))
    }
}

impl Workspace {
    /// Whether this workspace currently holds an epoch-0 group key (i.e. has been paired).
    pub fn has_group_key(&self) -> bool {
        self.key_store()
            .get(KeyId::Group(0))
            .ok()
            .flatten()
            .is_some()
    }

    /// TEST-ONLY: forces this workspace's sync group id and epoch-0 group key directly, bypassing
    /// the pairing handshake entirely. See the module doc for where the real guard lives.
    pub(crate) fn debug_set_group_key(
        &self,
        group: GroupId,
        key_bytes: Vec<u8>,
    ) -> Result<(), WorkspaceError> {
        self.key_store()
            .put(KeyId::Group(0), &Secret::new(key_bytes))?;
        let mut store = self
            .store()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        store.meta_set(GROUP_ID_KEY, &group.0.to_be_bytes())?;
        drop(store);
        self.set_group(group);
        Ok(())
    }
}
