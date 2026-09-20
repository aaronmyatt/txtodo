//! ratatui client (M10) binary entry point. All real logic lives in the `txtodo_tui` lib crate
//! (`src/lib.rs`) so it can be unit- and integration-tested without a terminal.
#![forbid(unsafe_code)]
#![allow(clippy::print_stdout)] // --version's only human-output path, same precedent as app.rs's own eprintln! doc

fn main() -> std::process::ExitCode {
    // No clap dependency here (this binary takes no other flags) — `RELEASE_CI.patch.md`'s own
    // smoke-test step (`for f in dist/*; do "./$f" --version; done`) runs against every shipped
    // binary, and without this check the real app loop below would run instead, attempting a
    // real daemon connection that fails loudly on a CI runner with no daemon — a real gap found
    // on this project's first tagged release, task `release-engineering`.
    if std::env::args().nth(1).as_deref() == Some("--version") {
        println!("txtodo-tui {}", txtodo_tui::buildinfo::VERSION_LINE);
        return std::process::ExitCode::SUCCESS;
    }
    txtodo_tui::app::main()
}
