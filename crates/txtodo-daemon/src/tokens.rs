//! Capability tokens (plan M6, design §6.2): `TokenCreate`/`TokenList`/`TokenRevoke`. Owned
//! end-to-end by the token-system task; the RPCs delegate here from `server.rs` untouched.

use crate::server::TxtodoService;
use tonic::{Request, Response, Status};
use txtodo_proto::v1 as pb;

impl TxtodoService {
    /// Mints a new capability token from the design §6.2 scope/caveat grammar.
    pub(crate) async fn token_create_impl(
        &self,
        _r: Request<pb::TokenCreateRequest>,
    ) -> Result<Response<pb::Token>, Status> {
        Err(Status::unimplemented(
            "token_create: the token store does not exist yet (M6)",
        ))
    }

    /// Lists tokens for this workspace, scopes included, secrets never returned.
    pub(crate) async fn token_list_impl(
        &self,
        _r: Request<pb::TokenListRequest>,
    ) -> Result<Response<pb::TokenListResponse>, Status> {
        Err(Status::unimplemented(
            "token_list: the token store does not exist yet (M6)",
        ))
    }

    /// Revokes a token; the daemon refuses it on its next use.
    pub(crate) async fn token_revoke_impl(
        &self,
        _r: Request<pb::TokenRevokeRequest>,
    ) -> Result<Response<pb::TokenRevokeResponse>, Status> {
        Err(Status::unimplemented(
            "token_revoke: the token store does not exist yet (M6)",
        ))
    }
}
