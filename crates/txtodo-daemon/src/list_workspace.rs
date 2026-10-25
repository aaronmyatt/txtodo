//! Named lists (task `two-kinds-of-workspace`, ADR 0043): the rules for a list's name and the
//! workspace id that name gives.
//!
//! A list is a workspace under `<data dir>/lists/<name>/` that txtodo syncs between the user's own
//! devices. Its id comes from its name, so two devices that both make `work` hold the same list with
//! no offer to accept, as they do for the default (ADR 0029). The derivation is permanent, like the
//! default's reserved id: changing it later is a migration, and `list_workspace_tests.rs` pins three
//! names to three ids so a change cannot slip through.

use txtodo_model::Ulid;
use txtodo_store::WorkspaceId;

/// The longest list name.
pub(crate) const MAX_LIST_NAME: usize = 48;

/// The timestamp field of every list id: fixed, non-zero and in the past, as the default's reserved
/// id has. Non-zero keeps a list id from ever being the all-zero `LINK_WORKSPACE` sentinel a sync
/// link seals its own hello under, and a fixed past instant keeps it from a minted ULID, whose
/// timestamp is the minting millisecond. It differs from the default's `0x019A_DEFA_0177`.
const LIST_ID_TIMESTAMP: u128 = 0x019A_115D_0001;

/// Names a filesystem refuses or treats as a device (Windows reserves these whatever the
/// extension), so a list folder can be made on any OS.
const RESERVED: [&str; 4] = ["con", "prn", "aux", "nul"];

/// A list's name after the checks: lowercase ASCII `[a-z0-9_-]`, 1 to [`MAX_LIST_NAME`] characters,
/// starting with a letter or digit, never `default`, never a Windows device name.
///
/// ASCII only, so no two devices disagree on Unicode normalisation and the folder name is safe on
/// every OS. No dots, so a folder moved aside as `<name>.removed-…` never collides with a name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ListName(String);

impl ListName {
    /// Trims and lowercases `raw`, then checks it. The error is the sentence a client shows.
    pub(crate) fn parse(raw: &str) -> Result<ListName, String> {
        let name = raw.trim().to_ascii_lowercase();
        if name.is_empty() || name.len() > MAX_LIST_NAME {
            return Err(format!(
                "a list name is 1 to {MAX_LIST_NAME} characters, got {}",
                name.chars().count()
            ));
        }
        let first_ok = name.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit());
        let rest_ok = name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-');
        if !first_ok || !rest_ok {
            return Err(format!(
                "list name {name:?} may use only a-z, 0-9, - and _, and starts with a letter or digit"
            ));
        }
        if name == "default" || is_device_name(&name) {
            return Err(format!("{name:?} is a reserved name"));
        }
        Ok(ListName(name))
    }

    /// The name as typed into a folder path and shown.
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    /// The workspace id this name gives: `blake3("txtodo-list:" + name)`, its low 80 bits under the
    /// fixed [`LIST_ID_TIMESTAMP`]. Every device computes the same one without asking.
    /// Ref: <https://docs.rs/blake3/latest/blake3/struct.Hasher.html>
    pub(crate) fn id(&self) -> WorkspaceId {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"txtodo-list:");
        hasher.update(self.0.as_bytes());
        let mut first = [0u8; 16];
        first.copy_from_slice(&hasher.finalize().as_bytes()[..16]);
        let random = u128::from_be_bytes(first) & ((1u128 << 80) - 1);
        WorkspaceId::new(Ulid::from_u128((LIST_ID_TIMESTAMP << 80) | random))
    }
}

/// `con`, `prn`, `aux`, `nul`, `com1`..`com9` and `lpt1`..`lpt9`.
fn is_device_name(name: &str) -> bool {
    let numbered = |prefix: &str| {
        name.strip_prefix(prefix)
            .is_some_and(|n| matches!(n.as_bytes(), [b'1'..=b'9']))
    };
    RESERVED.contains(&name) || numbered("com") || numbered("lpt")
}
