//! Named-list names and ids (`list_workspace.rs`, task two-kinds-of-workspace, ADR 0043).

use crate::default_workspace::default_workspace_id;
use crate::list_workspace::{ListName, MAX_LIST_NAME};
use txtodo_model::Ulid;

fn name(raw: &str) -> ListName {
    ListName::parse(raw).unwrap()
}

#[test]
fn a_name_is_trimmed_and_lowercased() {
    assert_eq!(name("  Work ").as_str(), "work");
    assert_eq!(name("my_list-2").as_str(), "my_list-2");
    assert_eq!(name("7").as_str(), "7");
    let longest = "a".repeat(MAX_LIST_NAME);
    assert_eq!(name(&longest).as_str(), longest);
}

#[test]
fn a_bad_name_is_refused_with_a_sentence() {
    let too_long = "a".repeat(MAX_LIST_NAME + 1);
    for raw in [
        "", "   ", &too_long, "-x", "_x", "a b", "a/b", "a.b", "..", "é", "wo\nrk", "a\\b",
    ] {
        let why = ListName::parse(raw).unwrap_err();
        assert!(!why.is_empty(), "{raw:?} was accepted");
    }
}

#[test]
fn the_default_and_windows_device_names_are_reserved() {
    for raw in [
        "default", "Default", "con", "NUL", "aux", "prn", "com1", "LPT9",
    ] {
        let why = ListName::parse(raw).unwrap_err();
        assert!(why.contains("reserved"), "{raw}: {why}");
    }
    // Close to a device name, but not one.
    for raw in ["com", "com0", "com10", "lpt", "console", "defaults"] {
        assert!(ListName::parse(raw).is_ok(), "{raw} is a fine name");
    }
}

#[test]
fn a_name_gives_one_id_whatever_its_case() {
    assert_eq!(name("work").id(), name("WORK").id());
    assert_eq!(name("work").id(), name(" work ").id());
    assert_ne!(name("work").id(), name("home").id());
}

#[test]
fn a_list_id_is_never_the_default_the_link_sentinel_or_a_minted_ulid_shape() {
    let id = name("work").id();
    assert_ne!(id, default_workspace_id());
    let bits = id.ulid().to_u128();
    assert_eq!(bits >> 80, 0x019A_115D_0001, "the fixed timestamp field");
    assert_ne!(bits, 0, "never the all-zero link sentinel");
    assert_ne!(bits >> 80, default_workspace_id().ulid().to_u128() >> 80);
    // The text form is a valid ULID the registry and the wire carry.
    assert_eq!(Ulid::parse(&id.to_string()), Some(id.ulid()));
}

/// The derivation is permanent: a change here is a migration for every list on every device.
#[test]
fn three_names_are_pinned_to_three_ids() {
    let pinned = [
        ("work", "01K88NT001XV65VHTNCYP6PE6F"),
        ("home", "01K88NT0015SHKXH8JFCPTKBMD"),
        ("a-b_c9", "01K88NT001NFC98TAGX7K9P9Q9"),
    ];
    for (raw, expected) in pinned {
        assert_eq!(name(raw).id().to_string(), expected, "{raw}");
    }
}
