//! ratatui client (M10). Design §7: "All clients are thin: they talk to `txtodod` over local IPC
//! and render. None of them parse the file themselves." This crate links `txtodo-core` directly
//! only for [`txtodo_core::tokenize`] (identical token boundaries everywhere, design §7); it never
//! opens `todo.txt` itself — every byte comes from the daemon's `GetFile`/`Watch`.
#![forbid(unsafe_code)]

pub mod buildinfo;
pub mod paint;
pub mod skill_hint;
pub mod state;

/// Widgets, each rendering a pure function of `&state::AppState` (design §7's per-row/pane split):
/// `list` (line list + vim nav), `edit` (single-line editor), `conflicts` (`r` pane), `sync`
/// (`s` indicator) and `screen` (the top-level frame composing all of them).
pub mod ui {
    pub mod conflicts;
    pub mod edit;
    pub mod list;
    pub mod screen;
    pub mod sync;
}

pub mod action;
pub mod daemon;
pub mod input;

pub mod app;
