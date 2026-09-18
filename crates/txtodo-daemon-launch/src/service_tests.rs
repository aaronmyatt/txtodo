//! `service.rs`'s tests — split out for its own file-length budget (`.claude/budgets.json`'s
//! `fileLines`), the same pattern the daemon crate's own `*_tests.rs` files use. Included via
//! `#[path]` so it stays part of the `service` module (`super::*` reaches every private item),
//! not a new top-level module needing its own `mod` line in `lib.rs`.

use super::*;

#[test]
fn render_is_one_global_unit_with_no_workspace_argument() {
    let body = render_template(
        LAUNCHD_TEMPLATE,
        LABEL,
        Path::new("/usr/local/bin/txtodod"),
        Path::new("/home/u/Library/Logs/txtodo"),
    );
    assert!(body.contains("<string>/usr/local/bin/txtodod</string>"));
    assert!(body.contains("<string>/home/u/Library/Logs/txtodo/launchd.out.log</string>"));
    assert!(
        !body.contains("<string>--dir</string>"),
        "true global mode takes no --dir argument"
    );
    assert!(!body.contains("{{"));
    let unit = render_template(
        SYSTEMD_TEMPLATE,
        LABEL,
        Path::new("/bin/txtodod"),
        Path::new("/home/u/Library/Logs/txtodo"),
    );
    assert_eq!(
        unit.lines().find(|l| l.starts_with("ExecStart=")),
        Some("ExecStart=/bin/txtodod")
    );
    let r = render(Path::new("/home/u"), Path::new("/bin/txtodod"));
    if cfg!(any(target_os = "macos", target_os = "linux")) {
        let r = r.unwrap_or_else(|| panic!("supported platform"));
        assert_eq!(r.label, LABEL);
        assert!(r.path.starts_with("/home/u"));
        assert_eq!(
            r.path.file_stem().and_then(|s| s.to_str()),
            Some(LABEL),
            "one bare label, no per-workspace hash suffix: {}",
            r.path.display()
        );
    }
}

#[test]
fn old_workspace_label_recognizes_the_pre_m11_hash_suffix_only() {
    assert_eq!(
        old_workspace_label("com.txtodo.txtodod.1a2b3c4d.plist", "plist"),
        Some("com.txtodo.txtodod.1a2b3c4d".to_owned())
    );
    assert_eq!(
        old_workspace_label("com.txtodo.txtodod.1a2b3c4d.service", "service"),
        Some("com.txtodo.txtodod.1a2b3c4d".to_owned())
    );
    // The new global unit's own file must never be mistaken for an old one to migrate.
    assert_eq!(
        old_workspace_label("com.txtodo.txtodod.plist", "plist"),
        None
    );
    assert_eq!(old_workspace_label("not-ours.plist", "plist"), None);
    assert_eq!(
        old_workspace_label("com.txtodo.txtodod.notquite8x.plist", "plist"),
        None,
        "wrong-length suffix is not a recognized old label"
    );
}

#[test]
fn migrate_old_units_removes_matching_files_and_leaves_the_new_one_alone() {
    let home = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let (dir, ext) = service_dir_and_ext(home.path())
        .unwrap_or_else(|| panic!("supported platform for this test"));
    std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("mkdir: {e}"));
    let old = dir.join(format!("{LABEL}.deadbeef.{ext}"));
    let new = dir.join(format!("{LABEL}.{ext}"));
    std::fs::write(&old, "old").unwrap_or_else(|e| panic!("write: {e}"));
    std::fs::write(&new, "new").unwrap_or_else(|e| panic!("write: {e}"));

    let migrated = migrate_old_units(home.path());
    assert_eq!(migrated, vec![format!("{LABEL}.deadbeef")]);
    assert!(!old.exists(), "the old per-workspace unit is removed");
    assert!(new.exists(), "the new global unit's own file is untouched");
}

#[test]
fn install_reports_migrated_units_and_the_written_path() {
    let home = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let r = render(home.path(), Path::new("/bin/txtodod"))
        .unwrap_or_else(|| panic!("supported platform for this test"));
    let outcome = install(home.path(), &r, false).unwrap_or_else(|e| panic!("install: {e}"));
    assert_eq!(outcome.path, r.path);
    assert!(outcome.migrated.is_empty(), "nothing to migrate yet");
    assert!(r.path.exists());

    let err = install(home.path(), &r, false)
        .expect_err("a second install without --force must refuse to clobber");
    assert!(err.to_string().contains("--force"));

    let forced = install(home.path(), &r, true).unwrap_or_else(|e| panic!("forced install: {e}"));
    assert_eq!(forced.path, r.path);
}

/// `installed_program_path` (task `daemon-stale-service-repair`): both templates' shapes,
/// round-tripped through `render_template` rather than hand-written XML/ini strings, so a future
/// template edit can't silently desync this parser from what `render` actually produces.
#[test]
fn installed_program_path_reads_back_what_render_template_wrote() {
    let plist = render_template(
        LAUNCHD_TEMPLATE,
        LABEL,
        Path::new("/usr/local/bin/txtodod"),
        Path::new("/home/u/Library/Logs/txtodo"),
    );
    assert_eq!(
        installed_program_path(&plist),
        Some(PathBuf::from("/usr/local/bin/txtodod"))
    );
    let unit = render_template(
        SYSTEMD_TEMPLATE,
        LABEL,
        Path::new("/bin/txtodod"),
        Path::new("/home/u/Library/Logs/txtodo"),
    );
    assert_eq!(
        installed_program_path(&unit),
        Some(PathBuf::from("/bin/txtodod"))
    );
    assert_eq!(installed_program_path("not a unit file at all"), None);
}

/// A freshly installed unit, pointing at a binary that really exists, is never stale — the common
/// case, and the one `install_persistent_service_best_effort` must not disturb.
#[test]
fn a_fresh_install_pointing_at_a_real_binary_is_not_stale() {
    let home = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let real_bin = home.path().join("txtodod");
    std::fs::write(&real_bin, b"#!/bin/sh\n").unwrap_or_else(|e| panic!("write bin: {e}"));
    let r = render(home.path(), &real_bin).unwrap_or_else(|| panic!("supported platform"));
    install(home.path(), &r, false).unwrap_or_else(|e| panic!("install: {e}"));

    assert!(!is_stale(&r));
}

/// The exact reported bug: a unit installed against a binary that has since been deleted (e.g. a
/// git worktree removed after the install) is stale, and reinstalling with `force` repairs it —
/// the same repair `ensure_daemon`'s best-effort install now performs automatically.
#[test]
fn a_unit_pointing_at_a_deleted_binary_is_stale_and_force_reinstall_repairs_it() {
    let home = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let gone_bin = home.path().join("worktree-txtodod");
    std::fs::write(&gone_bin, b"#!/bin/sh\n").unwrap_or_else(|e| panic!("write bin: {e}"));
    let stale = render(home.path(), &gone_bin).unwrap_or_else(|| panic!("supported platform"));
    install(home.path(), &stale, false).unwrap_or_else(|e| panic!("install: {e}"));
    std::fs::remove_file(&gone_bin).unwrap_or_else(|e| panic!("simulate worktree deletion: {e}"));

    assert!(is_stale(&stale), "the recorded binary no longer exists");

    let real_bin = home.path().join("real-txtodod");
    std::fs::write(&real_bin, b"#!/bin/sh\n").unwrap_or_else(|e| panic!("write real bin: {e}"));
    let repaired = render(home.path(), &real_bin).unwrap_or_else(|| panic!("supported platform"));
    assert_eq!(repaired.path, stale.path, "same unit, repaired in place");
    install(home.path(), &repaired, true).unwrap_or_else(|e| panic!("forced reinstall: {e}"));

    assert!(
        !is_stale(&repaired),
        "repaired unit now points at a real binary"
    );
}

/// No unit installed at all is "not installed", never "stale" — `is_stale` must not treat a
/// missing file as a repair opportunity (that's `install`'s own, unconditional job).
#[test]
fn no_installed_unit_is_not_stale() {
    let home = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let r = render(home.path(), Path::new("/bin/txtodod"))
        .unwrap_or_else(|| panic!("supported platform"));
    assert!(!r.path.exists());
    assert!(!is_stale(&r));
}
