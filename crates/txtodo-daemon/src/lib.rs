//! txtodod internals: document state, reconciler, actor, watcher, gRPC server. Plan M3.
//! The binary in `main.rs` only wires these together; everything here is testable in-process.
#![forbid(unsafe_code)]

mod activity;
pub mod actor;
mod actor_mirror;
mod apply_route;
mod bundle_crypto;
mod bundle_export;
mod bundle_grpc;
mod bundle_import;
mod bundle_import_error;
mod bundle_wire;
pub mod clock;
mod commit;
mod conflict_row;
pub mod convert;
pub mod debounce;
pub mod debug_hooks;
mod device_remove;
mod devices_grpc;
pub mod expected;
mod external;
pub mod fastid;
mod fields;
pub mod handle;
pub mod history;
mod identity_assign;
mod identity_fingerprint;
mod identity_levenshtein;
mod import;
mod keystore_setup;
pub mod lan;
mod lan_apply;
mod lan_peers;
mod lan_session;
pub mod lan_status;
pub mod mirror;
mod mirror_converge;
mod move_coordinator;
#[cfg(test)]
mod move_coordinator_tests;
pub mod mutation;
mod notes;
pub mod notes_actor;
pub mod notes_history;
pub mod notes_lookup;
pub mod notes_mirror;
pub mod notes_registry;
pub mod notes_state;
mod pairing_grpc;
mod pairing_lan;
mod pairing_lan_state;
mod pairing_state;
mod pairing_state_error;
mod pairing_wire;
pub mod pidfile;
mod progress;
pub mod reconcile;
pub mod reconcile_sidecar;
pub mod refdir;
mod refdir_grpc;
mod refdir_ops;
pub mod serve;
pub mod server;
mod server_actors;
pub mod state;
pub mod stats;
mod sync_ops;
pub mod telemetry;
pub mod textedit;
mod tokens;
mod tree;
pub mod tree_dirty;
pub mod walker;
mod watch_forward;
pub mod watch_task;
pub mod watcher;
pub mod workspace;
mod workspace_error;
mod workspace_mint;
pub mod workspace_registry;
mod workspace_registry_error;
pub mod workspace_registry_paths;
pub mod write;

#[cfg(test)]
mod actor_tests;
#[cfg(test)]
mod bundle_tests;
#[cfg(test)]
mod device_remove_tests;
#[cfg(test)]
mod history_tests;
#[cfg(test)]
mod import_tests;
#[cfg(test)]
mod lan_session_security_tests;
#[cfg(test)]
mod lan_session_tests;
#[cfg(test)]
mod mirror_tests;
#[cfg(test)]
mod mutation_tests;
#[cfg(test)]
mod notes_actor_tests;
#[cfg(test)]
mod pairing_grpc_tests;
#[cfg(test)]
mod reconcile_sidecar_tests;
#[cfg(test)]
mod reconcile_tests;
#[cfg(test)]
mod refdir_tests;
#[cfg(test)]
mod state_goldens;
#[cfg(test)]
mod state_tests;
#[cfg(test)]
mod sync_ops_tests;
#[cfg(test)]
mod workspace_registry_tests;
#[cfg(test)]
mod workspace_tests;
