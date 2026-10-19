//! The `[y/N]` prompt the destructive commands share: `conflicts delete`/`keep-newest`,
//! `workspace bind`/`rejoin`, and `pair`'s comparison. The prompt goes to stderr so `--json`
//! output stays clean, the default is no, and a closed stdin is a no.

use std::io::Write;

use crate::CliError;

/// Asks `question`, then reads one line from stdin. True only for an explicit `y` or `yes`
/// ([`is_yes`]); EOF is a no.
///
/// Ref: https://doc.rust-lang.org/std/io/struct.Stdin.html#method.read_line
pub fn confirm(question: &str) -> Result<bool, CliError> {
    eprint!("{question} [y/N] ");
    std::io::stderr().flush().ok();
    let mut answer = String::new();
    std::io::stdin()
        .read_line(&mut answer)
        .map_err(CliError::Io)?;
    Ok(is_yes(&answer))
}

/// The pure rule behind [`confirm`]: `y` or `yes`, case-insensitive, surrounding space ignored.
pub fn is_yes(answer: &str) -> bool {
    matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}
