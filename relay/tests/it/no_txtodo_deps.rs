//! The relay must never depend on any `txtodo-*` crate: it is untrusted (design §4.6) and
//! should be structurally incapable of importing the crypto that would let it read what it
//! stores. This test reads the crate's own manifest so the guarantee survives future edits.
#[test]
fn manifest_has_no_txtodo_dependency() {
    let manifest = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))
        .expect("read relay/Cargo.toml");
    assert!(
        !manifest.contains("txtodo-"),
        "relay/Cargo.toml must never reference a txtodo-* crate"
    );
}
