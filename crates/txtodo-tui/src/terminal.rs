//! Terminal setup (task `tui-revamp/tui-foundation`): on top of ratatui's raw mode and alternate
//! screen, the TUI turns on mouse reports and, where the terminal speaks it, the kitty keyboard
//! protocol. [`leave`] undoes all of it on a normal exit; a panic hook undoes it before the panic
//! message prints, so a crash never leaves the shell in raw mode with mouse reports on.
//!
//! Ctrl-c is not a signal here: raw mode turns it into a key, and `keymap` binds it to `app.quit`.
//! Ref: <https://docs.rs/ratatui/latest/ratatui/init/index.html>,
//! <https://docs.rs/crossterm/latest/crossterm/event/struct.PushKeyboardEnhancementFlags.html>

use std::io::{Write, stdout};
use std::sync::atomic::{AtomicBool, Ordering};

use crossterm::event::{
    DisableMouseCapture, EnableMouseCapture, KeyboardEnhancementFlags, PopKeyboardEnhancementFlags,
    PushKeyboardEnhancementFlags,
};
use crossterm::execute;

/// Whether [`enter`] pushed keyboard flags, so [`leave`] and the panic hook pop only what was
/// pushed. A static because the panic hook cannot borrow anything.
static PUSHED_FLAGS: AtomicBool = AtomicBool::new(false);

/// The kitty protocol level asked for. Disambiguate only: `Esc` arrives at once, and Ctrl with
/// Shift (`Ctrl-Shift-Space`) or with Enter (`Ctrl-Enter`) reaches the app at all. Text keys still
/// come as text, so `G` is still `G`. No release events: the loop reads presses only.
/// Ref: <https://sw.kovidgoyal.net/kitty/keyboard-protocol/#disambiguate-escape-codes>
const FLAGS: KeyboardEnhancementFlags = KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES;

/// Takes over the terminal. Call once, before the input reader thread starts: the keyboard
/// protocol query reads the terminal's answer from stdin, and would race that thread.
///
/// # Panics
/// When raw mode or the alternate screen cannot be entered (ratatui's own `init` contract).
pub fn enter() -> ratatui::DefaultTerminal {
    // ratatui::init enables raw mode, enters the alternate screen and installs a panic hook that
    // leaves both; the hook below runs first and undoes what this module adds on top.
    // Ref: https://docs.rs/ratatui/latest/ratatui/fn.init.html
    let terminal = ratatui::init();
    install_panic_hook();
    // Best effort: a terminal that refuses mouse reports or keyboard flags still works by key.
    let _ = execute!(stdout(), EnableMouseCapture);
    // Sends the protocol's query plus a primary device attributes query every terminal answers,
    // so a terminal without the protocol answers "no" rather than timing out.
    // Ref: https://docs.rs/crossterm/latest/crossterm/terminal/fn.supports_keyboard_enhancement.html
    if matches!(
        crossterm::terminal::supports_keyboard_enhancement(),
        Ok(true)
    ) && execute!(stdout(), PushKeyboardEnhancementFlags(FLAGS)).is_ok()
    {
        PUSHED_FLAGS.store(true, Ordering::SeqCst);
    }
    terminal
}

/// Gives the terminal back: keyboard flags, mouse reports, then raw mode and the alternate screen.
pub fn leave() {
    undo_extras();
    // Ref: https://docs.rs/ratatui/latest/ratatui/fn.restore.html
    ratatui::restore();
}

/// Pops the keyboard flags (if pushed) and stops mouse reports. Safe to call twice.
fn undo_extras() {
    let mut out = stdout();
    if PUSHED_FLAGS.swap(false, Ordering::SeqCst) {
        let _ = execute!(out, PopKeyboardEnhancementFlags);
    }
    let _ = execute!(out, DisableMouseCapture);
    let _ = out.flush();
}

/// Chains a hook in front of the one ratatui installed, so a panic undoes this module's extras,
/// then ratatui's hook leaves raw mode and prints the message.
/// Ref: https://doc.rust-lang.org/std/panic/fn.set_hook.html
fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        undo_extras();
        previous(info);
    }));
}
