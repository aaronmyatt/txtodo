//! Task `default-workspace`'s audit: nothing a device syncs or offers holds an absolute path, and
//! no synced name can collide between a case-sensitive and a case-insensitive filesystem. The
//! default lives at a different absolute path on every device, so a leaked path would be wrong on
//! every device but one.

use crate::bundle_export::{ExportCtx, export_into};
use crate::clock::FakeClock;
use crate::control_session::outbound_offers;
use crate::keystore_setup::load_or_mint_device_signing;
use crate::walker;
use crate::workspace::Workspace;
use crate::workspace_registry::WorkspaceRegistry;
use std::sync::{Arc, Mutex};

/// A workspace whose root sits several directories down, with a nested `ref:` list and notes.
fn nested_workspace() -> (tempfile::TempDir, std::path::PathBuf) {
    let base = tempfile::tempdir().unwrap();
    let root = base
        .path()
        .join("very")
        .join("unlikely-parent")
        .join("default");
    let sub = root.join("tasks").join("ship-it");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(root.join("todo.txt"), "top task ref:ship-it\n").unwrap();
    std::fs::write(sub.join("todo.txt"), "sub task\n").unwrap();
    std::fs::write(sub.join("notes.md"), "notes\n").unwrap();
    (base, root)
}

#[test]
fn every_synced_path_the_walker_finds_is_relative_with_forward_slashes() {
    let (_base, root) = nested_workspace();
    let paths = walker::walk(&root).unwrap();
    assert_eq!(paths.len(), 3);
    for p in &paths {
        let s = p.as_str();
        assert!(
            !s.starts_with('/') && !s.contains('\\') && !s.contains(':'),
            "{s}"
        );
        assert!(!s.contains("unlikely-parent"), "the root leaked into {s}");
    }
}

#[test]
fn a_workspace_offer_carries_the_folder_name_and_never_a_path() {
    let (_base, root) = nested_workspace();
    let registry_dir = tempfile::tempdir().unwrap();
    let mut registry = WorkspaceRegistry::open(&registry_dir.path().join("registry.db")).unwrap();
    registry.add(&root, &FakeClock::new(1_000)).unwrap();
    let offers = outbound_offers(&Mutex::new(registry));
    assert_eq!(offers.len(), 1);
    assert_eq!(offers[0].1, "default");
}

#[tokio::test]
async fn a_bundle_export_holds_no_absolute_path_in_the_clear() {
    let (_base, root) = nested_workspace();
    let ws = Workspace::open(&root, Arc::new(FakeClock::new(1_000))).unwrap();
    let signing = load_or_mint_device_signing(ws.key_store().as_ref()).unwrap();
    let ctx = ExportCtx {
        root: ws.root().to_path_buf(),
        store: ws.store().clone(),
        device: ws.device(),
        signing,
    };
    let mut frames: Vec<Vec<u8>> = Vec::new();
    export_into(&ctx, b"passphrase", &mut |data| {
        frames.push(data);
        Ok(())
    })
    .unwrap();
    let text: Vec<u8> = frames.concat();
    for needle in [
        root.to_string_lossy().as_bytes(),
        b"unlikely-parent".as_slice(),
    ] {
        assert!(
            !text.windows(needle.len()).any(|w| w == needle),
            "the workspace's own location appears in the bundle"
        );
    }
}

/// `ref:` slugs are lowercase (plan §3.2.1), so two folders that differ only in case cannot both
/// be named by a valid slug: a case-insensitive filesystem never sees them collide.
#[test]
fn an_uppercase_ref_slug_is_not_a_ref() {
    let file = txtodo_core::parse_file(b"task ref:Ship-It\nother ref:ship-it\n");
    let slugs: Vec<Option<String>> = file
        .lines
        .iter()
        .map(|l| match l.parse().map(|p| p.kind) {
            Some(txtodo_core::LineKind::Task(t)) => t.ref_slug().map(str::to_owned),
            _ => None,
        })
        .collect();
    assert_eq!(slugs, vec![None, Some("ship-it".to_owned())]);
}
