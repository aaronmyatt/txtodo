//! ratatui client (M10) binary entry point. All real logic lives in the `txtodo_tui` lib crate
//! (`src/lib.rs`) so it can be unit- and integration-tested without a terminal.
#![forbid(unsafe_code)]

fn main() -> std::process::ExitCode {
    txtodo_tui::app::main()
}
