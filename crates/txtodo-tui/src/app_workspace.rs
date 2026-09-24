//! Which workspace the TUI opens with no `--dir`, and the status-line label naming its kind. Split
//! out of `app.rs` for its line budget.

use txtodo_workspace_paths::{
    RegistryEnv, WorkspaceChoice, choose_workspace, remote_workspaces_dir_for,
};

/// The current folder when it is a workspace, else the user's default one (task
/// default-workspace), and the status-line label that says which kind it is.
pub fn pick_workspace(cwd: std::path::PathBuf) -> (std::path::PathBuf, Option<String>) {
    let Ok(env) = RegistryEnv::from_process() else {
        return (cwd, None);
    };
    let choice = choose_workspace(&env, &cwd);
    let label = workspace_label(&env, &choice);
    (choice.path().to_path_buf(), label)
}

/// `default workspace`, or `remote workspace` inside a mirror of a paired device's workspace (task
/// remote-workspace-mirror: the daemon keeps those under its own `remote/` folder), else none.
pub fn workspace_label(env: &RegistryEnv, choice: &WorkspaceChoice) -> Option<String> {
    if choice.is_default() {
        return Some("default workspace".to_owned());
    }
    // Canonical on both sides, like the daemon's registry (macOS `/var` is `/private/var`).
    // Ref: https://doc.rust-lang.org/std/fs/fn.canonicalize.html
    let canonical = |p: &std::path::Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    let remote = canonical(&remote_workspaces_dir_for(env));
    canonical(choice.path())
        .starts_with(remote)
        .then(|| "remote workspace".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    fn env(socket: &std::path::Path) -> RegistryEnv {
        let vars = BTreeMap::from([(
            "TXTODO_SOCKET".to_owned(),
            socket.join("txtodod.sock").display().to_string(),
        )]);
        RegistryEnv::new(vars, PathBuf::from("/"))
    }

    #[test]
    fn a_folder_inside_the_mirror_folder_is_labelled_remote() {
        let state = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
        let env = env(state.path());
        let mirror = remote_workspaces_dir_for(&env).join("01BX5ZZKBKACTAV9WEVGEMMVRZ");
        std::fs::create_dir_all(&mirror).unwrap_or_else(|e| panic!("mkdir: {e}"));
        let label = workspace_label(&env, &WorkspaceChoice::Here(mirror));
        assert_eq!(label.as_deref(), Some("remote workspace"));

        let own = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
        let label = workspace_label(&env, &WorkspaceChoice::Here(own.path().to_path_buf()));
        assert_eq!(label, None, "a folder the user picked says nothing");
    }
}
