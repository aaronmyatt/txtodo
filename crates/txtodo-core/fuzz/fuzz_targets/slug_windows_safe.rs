#![no_main]
//! Fuzz target: security-m4-review, "path traversal impossible via `ref:` (fuzz the slug
//! validator)". Extends `slug.rs`'s existing traversal-safety assertions with the properties the
//! task names explicitly that `slug.rs` doesn't check on their own terms: a NUL byte, a leading
//! dot (implied by `slug.rs`'s ASCII-head assertion but asserted directly here too since the task
//! names it), and — the one genuinely new check — a Windows-reserved device name (`CON`, `PRN`,
//! `AUX`, `NUL`, `COM1`-`COM9`, `LPT1`-`LPT9`). Windows reserves these regardless of any
//! extension (`con.txt` still names the `CON` device, not a file called `con.txt`), and
//! `is_valid_slug`'s charset permits `.`, so a slug like `con.txt` passes every check `slug.rs`
//! already runs.
use libfuzzer_sys::fuzz_target;
use txtodo_core::is_valid_slug;

/// Windows device names reserved regardless of case or trailing extension.
/// Ref: <https://learn.microsoft.com/en-us/windows/win32/fileio/naming-a-file#naming-conventions>
const WINDOWS_RESERVED: &[&str] = &[
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// True when `s` names a Windows-reserved device, ignoring any `.`-suffixed extension — a slug is
/// always lowercase ASCII already (`is_valid_slug`'s own charset), so no case-folding is needed.
fn is_windows_reserved(s: &str) -> bool {
    let base = s.split('.').next().unwrap_or(s);
    WINDOWS_RESERVED.contains(&base)
}

fuzz_target!(|data: &[u8]| {
    let Ok(s) = core::str::from_utf8(data) else {
        return;
    };
    if is_valid_slug(s) {
        assert!(
            !s.contains('/') && !s.contains('\\') && s != "." && s != "..",
            "traversal-safe: {s:?}"
        );
        assert!(!s.contains('\0'), "no NUL byte: {s:?}");
        assert!(!s.starts_with('.'), "no leading dot: {s:?}");
        assert!(
            !is_windows_reserved(s),
            "not a Windows-reserved name: {s:?}"
        );
    }
});
