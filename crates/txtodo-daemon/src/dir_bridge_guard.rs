//! A `txtodod --dir <dir>` bridge must not open a root this device's global daemon holds (task
//! mcp-dir-second-daemon). The bridge keeps its own registry (`<dir>/.txtodo/registry.db`), so its
//! own overlap check never sees the global one. On 2026-09-28 a bridge ran on a root the global
//! daemon had open: both wrote one op log and one set of files (each MCP write reconciled again as
//! an external edit, so logged twice) and both bound the same relay endpoint id.
//!
//! [`global_owner`] reads the global registry before the bridge takes its pid lock or creates
//! `<dir>/.txtodo/`. It fails open: an unreadable registry never stops a bridge from starting.

use std::path::{Path, PathBuf};

use crate::workspace_registry::WorkspaceRegistry;
use crate::workspace_registry_paths::{self, RegistryEnv, root_overlap};

/// The active workspace in the device's global registry that is `dir`, holds it, or sits inside
/// it: its id and root. `None` when the bridge shares that registry itself (`$TXTODO_REGISTRY_DB`
/// names one file for both), when no global registry exists, or when it cannot be read.
pub fn global_owner(env: &RegistryEnv, dir: &Path) -> Option<(String, PathBuf)> {
    let global = workspace_registry_paths::registry_db_path(env);
    let own = workspace_registry_paths::registry_db_path_for(env, Some(dir));
    if global == own || !global.is_file() {
        return None;
    }
    let entries = WorkspaceRegistry::open(&global)
        .and_then(|registry| registry.list())
        .ok()?;
    entries
        .into_iter()
        .find(|e| e.root == dir || root_overlap(&e.root, dir).is_some())
        .map(|e| (e.id.to_string(), e.root))
}
