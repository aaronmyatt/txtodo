//! The fresh-profile default workspace (task default-workspace-client-agreement), split out of
//! `e2e_bridge.rs` for its file-length budget: resolved through the same shared helper the daemon
//! uses, then cross-checked against the daemon's own `is_default` entry before any spec runs.

use std::path::{Path, PathBuf};

use desktop_lib::daemon::DaemonClient;

/// The default workspace directory the isolated daemon will reserve: `default_workspace_dir_for`
/// with the daemon's own `TXTODO_SOCKET` in view (this process sets it on the child only).
pub(crate) fn default_workspace_for(global_state_dir: &str) -> PathBuf {
    let mut vars: std::collections::BTreeMap<String, String> = std::env::vars().collect();
    vars.insert(
        "TXTODO_SOCKET".to_owned(),
        format!("{global_state_dir}/txtodod.sock"),
    );
    let env =
        txtodo_workspace_paths::RegistryEnv::new(vars, std::env::current_dir().unwrap_or_default());
    txtodo_workspace_paths::default_workspace_dir_for(&env)
}

/// Fails loud, before any spec runs, if the daemon's `is_default` root is not the directory this
/// bridge computed: a drift here would auto-register the bridge's guess as an ordinary workspace
/// and every `fresh` assertion would fail confusingly.
pub(crate) async fn assert_default_agrees(sock: &Path, workspace: &str) {
    let mut client = DaemonClient::connect(sock, None)
        .await
        .unwrap_or_else(|e| panic!("e2e_bridge: connect for the default check: {e}"));
    client
        .wait_until_ready()
        .await
        .unwrap_or_else(|e| panic!("e2e_bridge: wait_until_ready: {e}"));
    let listed = client
        .workspace_list()
        .await
        .unwrap_or_else(|e| panic!("e2e_bridge: workspace_list: {e}"));
    let daemon_default = listed
        .iter()
        .find(|w| w.is_default)
        .map(|w| w.root.clone())
        .unwrap_or_else(|| panic!("e2e_bridge: the daemon reserved no default workspace"));
    let same = |a: &str, b: &str| {
        let canon = |p: &str| {
            Path::new(p)
                .canonicalize()
                .unwrap_or_else(|_| PathBuf::from(p))
        };
        a == b || canon(a) == canon(b)
    };
    assert!(
        same(&daemon_default, workspace),
        "e2e_bridge: the daemon's default workspace is {daemon_default} but this bridge computed {workspace}"
    );
}
