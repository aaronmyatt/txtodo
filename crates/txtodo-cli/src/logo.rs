//! The txtodo logo in plain ASCII (assets/brand/tx-mark.svg: a t whose bar hooks down like a check,
//! then an x), shown above `txtodo --help`. Plain ASCII and no colour, so a pipe or `less` shows it
//! as it is. `-h` leaves it out: clap shows `before_long_help` only for the long help.
//! Ref: <https://docs.rs/clap/latest/clap/struct.Command.html#method.before_long_help>

/// Seven rows, the t's foot and the x's legs on the last one.
pub const LOGO: &str = r"    |
 ---+--.
    |   \     \   /
    |    `     \ /
    |           X
    |          / \
     `--      /   \";

#[cfg(test)]
mod tests {
    use crate::cli::Cli;
    use clap::CommandFactory;

    // CommandFactory::command builds the clap Command the derive describes.
    // Ref: <https://docs.rs/clap/latest/clap/trait.CommandFactory.html>
    #[test]
    fn long_help_opens_with_the_logo_and_short_help_does_not() {
        let long = Cli::command().render_long_help().to_string();
        let short = Cli::command().render_help().to_string();
        assert!(long.starts_with(super::LOGO), "{long}");
        assert!(!short.contains(super::LOGO), "{short}");
        assert!(super::LOGO.is_ascii());
    }
}
