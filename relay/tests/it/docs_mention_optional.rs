//! tasks/docs-relay-selfhost acceptance: "assert README.md and docs/relay.md both contain the
//! word 'optional' so no reader concludes the relay is required" (design §4.5: "LAN alone is a
//! complete system").
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::path::PathBuf;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

#[test]
fn readme_and_relay_doc_both_say_optional() {
    let readme =
        std::fs::read_to_string(workspace_root().join("README.md")).expect("read README.md");
    let relay_doc = std::fs::read_to_string(workspace_root().join("docs/relay.md"))
        .expect("read docs/relay.md");

    assert!(
        readme.contains("optional"),
        "README.md must state the relay is optional"
    );
    assert!(
        relay_doc.contains("optional"),
        "docs/relay.md must state the relay is optional"
    );
}
