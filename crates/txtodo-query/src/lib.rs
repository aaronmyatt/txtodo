//! Query language: parse, plan, evaluate.
//!
//! The design §8 grammar is not built yet. Until it is, the line search every client shares is
//! `txtodo_core::query`, re-exported here so a crate that may reach this one but not core (MCP,
//! `.claude/budgets.json` `allowedDeps`) matches exactly as `txtodo list`, the TUI and desktop do
//! (task `tui-revamp/shared-core`, 2026-09-25).
#![forbid(unsafe_code)]

pub use txtodo_core::query::{GOLDEN, matches, matches_terms};
