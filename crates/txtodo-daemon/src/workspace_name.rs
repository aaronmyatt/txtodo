//! A workspace's shown name (task workspace-vanity-name): the one name every paired device shows
//! for it, instead of a mirror folder named by its id. Display only; ids stay ULIDs.
//!
//! Stored as a top-level `name` in `<root>/txtodo.toml`, the file that already holds the layout and
//! already syncs as a whole-text document (`layout_sync.rs`). So a name set on one device reaches
//! every device holding the workspace with no new wire field and no registry column.
//!
//! ```toml
//! name = "Groceries"   # optional; shown instead of the folder name
//! ```
//!
//! The file merges like notes.md, character by character, so two names set at once on two devices
//! can leave two `name` lines, which TOML refuses. Every device then holds the same text and reads
//! the same, last, line ([`parse`]), and the layout reads past both (`layout_file::parse`). The
//! next [`with_name`] writes one line again.
//!
//! TOML basic strings: <https://toml.io/en/v1.0.0#string>

use serde::Deserialize;
use std::path::Path;
use txtodo_store::WorkspaceId;

use crate::layout_file::LAYOUT_FILE;

/// Most bytes a name may have: the most a control-channel offer carries, so a set name always fits
/// the offer that names the workspace to a peer.
pub(crate) const MAX_NAME_BYTES: usize = txtodo_sync::MAX_WORKSPACE_NAME_BYTES;

/// What the default workspace is called until someone names it (ADR 0029).
const DEFAULT_NAME: &str = "default";

#[derive(Deserialize, Default)]
struct Raw {
    name: Option<String>,
}

/// `raw` as a name: trimmed, `None` when that leaves nothing. Refused, with the reason, when it
/// holds a control character (a tab, a new line) or is longer than [`MAX_NAME_BYTES`].
pub(crate) fn clean(raw: &str) -> Result<Option<String>, String> {
    let name = raw.trim();
    if name.is_empty() {
        return Ok(None);
    }
    if name.chars().any(char::is_control) {
        return Err(
            "a workspace name cannot hold a tab, a new line or another control character"
                .to_owned(),
        );
    }
    if name.len() > MAX_NAME_BYTES {
        return Err(format!(
            "a workspace name is at most {MAX_NAME_BYTES} bytes; this one is {}",
            name.len()
        ));
    }
    Ok(Some(name.to_owned()))
}

/// The name a `txtodo.toml`'s text sets, when it holds a valid one. Unknown keys are ignored, like
/// `layout_file::parse`. Text that is not valid TOML (two renames at once left two `name` lines,
/// which TOML refuses) is named by its last top-level `name` line.
pub(crate) fn parse(text: &str) -> Option<String> {
    let raw: Raw = toml::from_str(text).ok().or_else(|| last_name_line(text))?;
    clean(&raw.name?).ok().flatten()
}

/// The last top-level `name = ...` line of `text`, read on its own.
fn last_name_line(text: &str) -> Option<Raw> {
    let line = text
        .split_inclusive('\n')
        .take_while(|l| !l.trim_start().starts_with('['))
        .filter(|l| is_name_line(l.trim_start()))
        .last()?;
    toml::from_str(line).ok()
}

/// The name `<root>/txtodo.toml` sets; `None` when there is no file, no name, or no valid one.
pub(crate) fn read(root: &Path) -> Option<String> {
    parse(&std::fs::read_to_string(root.join(LAYOUT_FILE)).ok()?)
}

/// `text` (a `txtodo.toml`) with its top-level `name` set to `name`, or removed for `None`. Only
/// that line changes: it stands where the first `name` line stood (any others go), else before the
/// first `[table]` header, else at the end. Refused when the result would not read back as `name`
/// (the file is not valid TOML, or holds the key in a form this edit does not see).
pub(crate) fn with_name(text: &str, name: Option<&str>) -> Result<String, String> {
    let line = name.map(|n| format!("name = {}\n", quoted(n)));
    let out = put_name_line(text, line.as_deref());
    check_reads_back(&out, name)?;
    Ok(out)
}

/// `text` with no top-level `name` line: what the layout reads, and a rename's first step.
pub(crate) fn without_name(text: &str) -> String {
    put_name_line(text, None)
}

/// The texts a rename of `text` to `name` goes through, each to be one edit: the old `name`
/// line(s) out, then the new one in. Never one diff from name to name, which would splice two
/// renames made at once into one name; this way they leave two whole lines ([`parse`]). Empty
/// when `text` already names `name`.
pub(crate) fn rename_steps(text: &str, name: Option<&str>) -> Result<Vec<String>, String> {
    let named = with_name(text, name)?;
    if named == text {
        return Ok(Vec::new());
    }
    let mut steps: Vec<String> = [without_name(text), named]
        .into_iter()
        .filter(|step| step != text)
        .collect();
    // Clearing a name makes both steps the same text; `dedup` keeps one.
    // Ref: https://doc.rust-lang.org/std/vec/struct.Vec.html#method.dedup
    steps.dedup();
    Ok(steps)
}

/// `text` with every top-level `name = ...` line dropped, and `line` (when given) put where the
/// first of them stood, else before the first `[table]` header, else at the end. Every other byte
/// stays as it was.
fn put_name_line(text: &str, line: Option<&str>) -> String {
    let mut line = line;
    let mut out = String::with_capacity(text.len() + line.map_or(0, str::len));
    let mut in_table = false;
    // `split_inclusive` keeps each line's own ending, so untouched lines stay byte for byte.
    // Ref: https://doc.rust-lang.org/std/primitive.str.html#method.split_inclusive
    for l in text.split_inclusive('\n') {
        let trimmed = l.trim_start();
        in_table |= trimmed.starts_with('[');
        let is_name = !in_table && is_name_line(trimmed);
        if is_name || in_table {
            // `Option::take` hands the line out once. Ref:
            // https://doc.rust-lang.org/std/option/enum.Option.html#method.take
            out.push_str(line.take().unwrap_or_default());
        }
        if !is_name {
            out.push_str(l);
        }
    }
    if let Some(line) = line {
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(line);
    }
    out
}

/// `with_name`'s guard: the edited text must parse, and name exactly `name`.
fn check_reads_back(text: &str, name: Option<&str>) -> Result<(), String> {
    let raw: Raw = toml::from_str(text)
        .map_err(|e| format!("{LAYOUT_FILE} is not valid TOML, so its name was left alone: {e}"))?;
    if raw.name.as_deref() != name {
        return Err(format!(
            "{LAYOUT_FILE} holds `name` in a form this edit does not change; edit it by hand"
        ));
    }
    Ok(())
}

/// A top-level `name = ...` line (after its indent is trimmed); not `names`, not `name_x`.
fn is_name_line(trimmed: &str) -> bool {
    trimmed
        .strip_prefix("name")
        .is_some_and(|rest| rest.trim_start().starts_with('='))
}

/// The name a client shows for a workspace: the name its `txtodo.toml` sets; else `default` for
/// the default workspace; else its folder's name, unless that folder is named by the workspace id
/// (a mirror, `remote/<id>/`), which tells a person nothing, so what the offering device calls it
/// (`offered`) wins there when it said anything.
pub(crate) fn display_name(
    id: WorkspaceId,
    root: &Path,
    is_default: bool,
    offered: impl FnOnce() -> Option<String>,
) -> String {
    if let Some(name) = read(root) {
        return name;
    }
    if is_default {
        return DEFAULT_NAME.to_owned();
    }
    let folder = root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if (folder.is_empty() || folder == id.to_string())
        && let Some(name) = offered()
    {
        return name;
    }
    if folder.is_empty() {
        return root.display().to_string();
    }
    folder
}

/// Whether an offered `name` says anything about workspace `id`: a device that has no name for a
/// mirror offers its folder's name, which is the id itself.
pub(crate) fn offered_name(id: WorkspaceId, name: &str) -> Option<String> {
    clean(name).ok().flatten().filter(|n| *n != id.to_string())
}

/// `name` as a TOML basic string. [`clean`] already refused control characters, so a backslash and
/// a double quote are the only two left that need an escape.
fn quoted(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 2);
    out.push('"');
    for c in name.chars() {
        if c == '"' || c == '\\' {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}
