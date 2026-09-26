//! txtodod internals: document state, reconciler, actor, watcher, gRPC server. Plan M3.
//! The binary in `main.rs` only wires these together; everything here is testable in-process.
#![forbid(unsafe_code)]

mod activity;
pub mod actor;
mod actor_apply;
mod actor_mirror;
mod apply_route;
pub mod args_parse;
pub mod buildinfo;
mod bundle_crypto;
mod bundle_export;
mod bundle_grpc;
mod bundle_import;
mod bundle_import_error;
mod bundle_wire;
pub mod clock;
mod commit;
mod conflict_row;
mod conflicts_grpc;
mod contents;
pub mod control_channel;
mod control_dispatch;
mod control_session;
pub mod convert;
pub mod debounce;
pub mod debug_hooks;
mod default_workspace;
pub mod device_identity;
pub mod device_lan;
pub mod device_relay;
mod device_remove;
mod devices_grpc;
pub mod expected;
mod external;
pub mod fastid;
mod fields;
pub mod file_carrier;
pub mod global_service;
mod global_service_helpers;
pub mod handle;
mod handle_apply;
mod health_grpc;
pub mod history;
mod id_strip;
mod identity_assign;
mod identity_fingerprint;
mod identity_levenshtein;
mod import;
mod join_target;
mod keystore_cache;
mod keystore_setup;
mod keystore_timeout;
pub mod lan;
mod lan_apply;
mod lan_peers;
mod lan_session;
mod lan_session_dispatch;
mod lan_session_gate;
mod lan_session_live;
mod lan_session_ops;
mod lan_session_shared;
pub mod lan_status;
pub mod layout_file;
mod layout_reload;
mod layout_rpc;
pub mod layout_state;
mod layout_sync;
mod live_peers;
mod migrate_grpc;
pub mod migrate_sidecar;
pub mod mirror;
mod mirror_converge;
mod move_coordinator;
#[cfg(test)]
mod move_coordinator_tests;
pub mod mutation;
mod mutation_moves;
mod mutation_reopen;
mod notes;
pub mod notes_actor;
pub mod notes_history;
pub mod notes_lookup;
pub mod notes_mirror;
pub mod notes_registry;
pub mod notes_state;
mod pairing_adopt;
mod pairing_group_key;
mod pairing_grpc;
mod pairing_lan;
mod pairing_lan_reject;
mod pairing_lan_state;
mod pairing_register;
mod pairing_relay_dial;
mod pairing_state;
mod pairing_state_error;
mod pairing_wire;
mod peer_keys;
pub mod pidfile;
mod progress;
pub mod reconcile;
mod reconcile_replay;
pub mod reconcile_sidecar;
pub mod refdir;
mod refdir_grpc;
mod refdir_ops;
pub mod relay;
mod relay_autodial;
mod relay_fallback;
#[cfg(test)]
mod relay_fallback_tests;
mod relay_state;
mod replace;
pub mod runtime_exit;
pub mod serve;
pub mod server;
mod server_actors;
pub mod state;
mod state_error;
pub mod stats;
mod stored_ids;
mod stuck_sync;
mod sync_ops;
pub mod telemetry;
pub mod textedit;
mod tokens;
mod tree;
pub mod tree_dirty;
mod unified_diff;
mod universal_grpc;
pub mod walker;
mod watch_forward;
pub mod watch_task;
pub mod watcher;
pub mod workspace;
pub mod workspace_catalog;
mod workspace_catalog_load;
mod workspace_catalog_mirror;
mod workspace_catalog_offers;
mod workspace_catalog_open;
mod workspace_discover;
mod workspace_error;
pub mod workspace_load;
mod workspace_migrate;
mod workspace_mint;
mod workspace_offer_grpc;
pub mod workspace_offer_registry;
pub mod workspace_registry;
mod workspace_registry_error;
/// Re-exported from the shared `txtodo-workspace-paths` crate (task `daemon-paths-shared-crate`)
/// so every existing `txtodo_daemon::workspace_registry_paths::*` call site keeps working
/// unchanged — `txtodo-cli`/`txtodo-mcp`/`apps/desktop` now depend on the same crate directly
/// instead of each reimplementing its fallback chain.
pub use txtodo_workspace_paths as workspace_registry_paths;
pub mod write;

#[cfg(test)]
mod actor_tests;
#[cfg(test)]
mod bundle_tests;
#[cfg(test)]
mod control_dispatch_tests;
#[cfg(test)]
mod control_session_tests;
#[cfg(test)]
mod default_workspace_audit_tests;
#[cfg(test)]
mod default_workspace_tests;
#[cfg(test)]
mod device_relay_tests;
#[cfg(test)]
mod device_remove_tests;
#[cfg(test)]
mod devices_grpc_tests;
#[cfg(test)]
mod history_tests;
#[cfg(test)]
mod import_tests;
#[cfg(test)]
mod keystore_setup_tests;
#[cfg(test)]
mod lan_peers_tests;
#[cfg(test)]
mod lan_session_dup_tests;
#[cfg(test)]
mod lan_session_fairness_tests;
#[cfg(test)]
mod lan_session_gate_tests;
#[cfg(test)]
mod lan_session_push_tests;
#[cfg(test)]
mod lan_session_resend_tests;
#[cfg(test)]
mod lan_session_security_tests;
#[cfg(test)]
mod lan_session_tests;
#[cfg(test)]
mod layout_file_tests;
#[cfg(test)]
mod layout_refs_tests;
#[cfg(test)]
mod layout_reload_tests;
#[cfg(test)]
mod migrate_sidecar_tests;
#[cfg(test)]
mod mirror_tests;
#[cfg(test)]
mod mutation_reopen_tests;
#[cfg(test)]
mod mutation_tests;
#[cfg(test)]
mod mutation_tests_lines;
#[cfg(test)]
mod notes_actor_tests;
#[cfg(test)]
mod pairing_grpc_tests;
#[cfg(test)]
mod pairing_lan_tests;
#[cfg(test)]
mod peer_keys_dial_tests;
#[cfg(test)]
mod peer_keys_tests;
#[cfg(test)]
mod reconcile_replay_tests;
#[cfg(test)]
mod reconcile_sidecar_tests;
#[cfg(test)]
mod reconcile_tests;
#[cfg(test)]
mod refdir_tests;
#[cfg(test)]
mod security_m8_tests;
#[cfg(test)]
mod state_goldens;
#[cfg(test)]
mod state_tests;
#[cfg(test)]
mod status_details_tests;
#[cfg(test)]
mod stored_ids_tests;
#[cfg(test)]
mod stuck_sync_session_tests;
#[cfg(test)]
mod sync_ops_tests;
#[cfg(test)]
mod unified_diff_tests;
#[cfg(test)]
mod universal_grpc_tests;
#[cfg(test)]
mod workspace_catalog_load_tests;
#[cfg(test)]
mod workspace_catalog_mirror_tests;
#[cfg(test)]
mod workspace_catalog_state_tests;
#[cfg(test)]
mod workspace_catalog_tests;
#[cfg(test)]
mod workspace_migrate_tests;
#[cfg(test)]
mod workspace_offer_grpc_tests;
#[cfg(test)]
mod workspace_offer_registry_tests;
#[cfg(test)]
mod workspace_overlap_tests;
#[cfg(test)]
mod workspace_registry_tests;
#[cfg(test)]
mod workspace_tests;
