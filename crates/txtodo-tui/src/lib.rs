//! ratatui client (M10). Design §7: "All clients are thin: they talk to `txtodod` over local IPC
//! and render. None of them parse the file themselves." This crate links `txtodo-core` directly
//! only for [`txtodo_core::tokenize`] (identical token boundaries everywhere, design §7); it never
//! opens `todo.txt` itself — every byte comes from the daemon's `GetFile`/`Watch`.
#![forbid(unsafe_code)]

pub mod buildinfo;
pub mod clipboard;
pub mod hit;
pub mod paint;
pub mod skill_hint;
pub mod state;
pub mod state_detail;
pub mod state_nav;
pub mod state_offers;
pub mod state_shell;
pub mod state_tasks;
pub mod state_types;
pub mod state_universal;
pub mod terminal;
pub mod theme;

/// Widgets, each rendering a pure function of `&state::AppState` (design §7's per-row/pane split):
/// `list` (line list + vim nav), `edit` (single-line editor), `conflicts` (`r` pane), `sync`
/// (`s` indicator) and `screen` (the top-level frame composing all of them).
pub mod ui {
    pub mod banner;
    pub mod conflict_sheet;
    pub mod conflicts;
    pub mod detail;
    pub mod edit;
    pub mod footer;
    pub mod header;
    pub mod list;
    pub mod mark;
    pub mod notes_edit;
    pub mod offers;
    pub mod prompt_bar;
    pub mod row;
    pub mod screen;
    pub mod search_panel;
    pub mod subbar;
    pub mod sync;
    pub mod toast;
    pub mod universal;
    pub mod workspace_menu;
}

pub mod action;
pub mod commands;
pub mod commands_detail;
pub mod commands_nav;
pub mod commands_universal;
pub mod daemon;
pub mod daemon_activity;
pub mod daemon_devices;
pub mod daemon_history;
pub mod daemon_notes;
pub mod daemon_tokens;
pub mod daemon_workspace;
pub mod input;
pub mod input_detail;
pub mod keymap;
pub mod mouse;
pub mod prompt;
pub mod search;
pub mod search_suggest;

pub mod app;
pub mod app_apply;
pub mod app_detail;
pub mod app_layout;
pub mod app_loop;
pub mod app_offers;
pub mod app_refs;
pub mod app_universal;
pub mod app_workspace;
