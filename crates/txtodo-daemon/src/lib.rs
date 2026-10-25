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
use txtodo_engine::conflict_row;
mod conflicts_grpc;
use txtodo_engine::contents;
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
pub mod dir_bridge_guard;
#[cfg(test)]
mod dir_bridge_guard_tests;
mod list_rpc;
mod list_workspace;
mod mirrors_retire;
mod repo_exclude;
mod repo_git;
mod repo_scan;
mod repos_rpc;
pub use txtodo_engine::duplicates;
pub mod expected;
mod external;
pub use txtodo_engine::fastid;
pub mod file_carrier;
pub mod global_service;
mod global_service_helpers;
pub mod handle;
mod handle_apply;
mod handle_sync;
mod health_grpc;
pub mod history;
use txtodo_engine::id_strip;
use txtodo_engine::identity_fingerprint;
mod import;
mod join_target;
mod keystore_cache;
mod keystore_setup;
mod keystore_timeout;
pub mod lan;
mod lan_apply;
mod lan_peers;
mod lan_serve_merged;
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
pub use txtodo_engine::local_date;
mod log_repair;
mod migrate_grpc;
pub mod migrate_sidecar;
mod move_coordinator;
#[cfg(test)]
mod move_coordinator_tests;
pub use txtodo_engine::mutation;
use txtodo_engine::mutation_moves;
mod notes;
pub mod notes_actor;
pub mod notes_history;
pub mod notes_lookup;
pub mod notes_mirror;
pub mod notes_registry;
mod notes_repair;
pub use txtodo_engine::notes_state;
pub mod notices;
pub use txtodo_engine::op_set_hash;
mod own_vouch;
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
mod peer_clock;
mod peer_keys;
mod pending_save;
pub mod pidfile;
mod progress;
pub use txtodo_engine::reconcile;
mod reconcile_replay;
pub use txtodo_engine::reconcile_sidecar;
#[cfg(test)]
mod reconcile_skew_tests;
pub mod refdir;
mod refdir_grpc;
mod refdir_ops;
mod rejoin_backup;
pub mod relay;
mod relay_autodial;
mod relay_fallback;
#[cfg(test)]
mod relay_fallback_tests;
mod relay_state;
mod replace;
pub mod runtime_exit;
mod save_base;
pub mod serve;
pub mod server;
mod server_actors;
mod split_files;
use txtodo_engine::stamp_floor;
pub use txtodo_engine::state;
pub mod stats;
mod stored_ids;
mod stuck_sync;
mod sync_commit_gate;
mod sync_digest;
mod sync_ops;
use txtodo_engine::sync_park;
pub mod telemetry;
pub use txtodo_engine::textedit;
mod tokens;
mod tree;
pub mod tree_dirty;
use txtodo_engine::unified_diff;
mod universal_grpc;
pub mod walker;
mod watch_forward;
#[cfg(test)]
mod watch_opt_in;
pub mod watch_task;
pub mod watcher;
pub mod workspace;
pub mod workspace_bind;
pub mod workspace_catalog;
mod workspace_catalog_lists;
mod workspace_catalog_load;
mod workspace_catalog_mirror;
mod workspace_catalog_offers;
mod workspace_catalog_open;
pub mod workspace_catalog_repos;
mod workspace_discover;
mod workspace_error;
pub mod workspace_load;
mod workspace_migrate;
mod workspace_mint;
mod workspace_name;
mod workspace_offer_grpc;
pub mod workspace_offer_registry;
pub mod workspace_registry;
mod workspace_registry_error;
mod workspace_registry_grpc;
pub mod workspace_rejoin;
mod workspace_rename;
/// Re-exported from the shared `txtodo-workspace-paths` crate (task `daemon-paths-shared-crate`)
/// so every existing `txtodo_daemon::workspace_registry_paths::*` call site keeps working
/// unchanged — `txtodo-cli`/`txtodo-mcp`/`apps/desktop` now depend on the same crate directly
/// instead of each reimplementing its fallback chain.
pub use txtodo_workspace_paths as workspace_registry_paths;
pub mod write;
mod write_defer;

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
mod first_sync_bench_tests;
#[cfg(test)]
mod history_tests;
#[cfg(test)]
mod keystore_setup_tests;
#[cfg(test)]
mod lan_peers_tests;
#[cfg(test)]
mod lan_serve_merged_tests;
#[cfg(test)]
mod lan_session_dup_tests;
#[cfg(test)]
mod lan_session_fairness_tests;
#[cfg(test)]
mod lan_session_gate_tests;
#[cfg(test)]
mod lan_session_mesh_tests;
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
mod list_offer_tests;
#[cfg(test)]
mod list_rpc_tests;
#[cfg(test)]
mod list_workspace_tests;
#[cfg(test)]
mod log_repair_tests;
#[cfg(test)]
mod migrate_sidecar_tests;
#[cfg(test)]
mod mirrors_retire_tests;
#[cfg(test)]
mod notes_actor_sync_tests;
#[cfg(test)]
mod notes_actor_tests;
#[cfg(test)]
mod notes_clock_tests;
#[cfg(test)]
mod notes_held_tests;
#[cfg(test)]
mod op_set_hash_tests;
#[cfg(test)]
mod pairing_grpc_tests;
#[cfg(test)]
mod pairing_lan_tests;
#[cfg(test)]
mod peer_keys_dial_tests;
#[cfg(test)]
mod peer_keys_tests;
#[cfg(test)]
mod peer_protocol_tests;
#[cfg(test)]
mod pending_save_sidecar_tests;
#[cfg(test)]
mod pending_save_tests;
#[cfg(test)]
mod reconcile_replay_tests;
#[cfg(test)]
mod refdir_tests;
#[cfg(test)]
mod remove_tag_tests;
#[cfg(test)]
mod security_m8_tests;
#[cfg(test)]
mod status_details_tests;
#[cfg(test)]
mod stored_ids_tests;
#[cfg(test)]
mod stuck_sync_session_tests;
#[cfg(test)]
mod sync_commit_gate_tests;
#[cfg(test)]
mod sync_digest_tests;
#[cfg(test)]
mod sync_ops_tests;
#[cfg(test)]
mod universal_grpc_tests;
#[cfg(test)]
mod workspace_bind_tests;
#[cfg(test)]
mod workspace_catalog_load_tests;
#[cfg(test)]
mod workspace_catalog_mirror_tests;
#[cfg(test)]
mod workspace_catalog_repos_tests;
#[cfg(test)]
mod workspace_catalog_state_tests;
#[cfg(test)]
mod workspace_catalog_tests;
#[cfg(test)]
mod workspace_migrate_tests;
#[cfg(test)]
mod workspace_name_tests;
#[cfg(test)]
mod workspace_offer_grpc_tests;
#[cfg(test)]
mod workspace_offer_registry_tests;
#[cfg(test)]
mod workspace_overlap_tests;
#[cfg(test)]
mod workspace_registry_tests;
#[cfg(test)]
mod workspace_rejoin_tests;
#[cfg(test)]
mod workspace_rename_tests;
#[cfg(test)]
mod workspace_tests;
#[cfg(test)]
mod write_defer_tests;
