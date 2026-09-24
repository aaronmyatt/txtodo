//! Copy to the system clipboard from a terminal (task `tui-revamp/tui-shell`): the OSC 52 escape,
//! which the terminal (not this process) turns into a clipboard write, so it works over ssh too.
//! Terminals that ignore it copy nothing and say nothing. The payload is base64, encoded here
//! rather than through a crate for twenty lines.
//! Ref: <https://invisible-island.net/xterm/ctlseqs/ctlseqs.html#h3-Operating-System-Commands>,
//! <https://www.rfc-editor.org/rfc/rfc4648#section-4>

use std::io::Write;

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// RFC 4648 base64, padded.
pub fn base64(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// The escape that asks the terminal to put `text` on the clipboard (`c`).
pub fn osc52(text: &str) -> String {
    format!("\u{1b}]52;c;{}\u{7}", base64(text.as_bytes()))
}

/// Writes [`osc52`] for `text` to stdout, where the terminal reads it.
pub fn copy(text: &str) -> std::io::Result<()> {
    let mut out = std::io::stdout();
    out.write_all(osc52(text).as_bytes())?;
    out.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_the_rfc_vectors() {
        // RFC 4648 §10.
        let cases = [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ];
        for (plain, encoded) in cases {
            assert_eq!(base64(plain.as_bytes()), encoded, "{plain:?}");
        }
    }

    #[test]
    fn the_escape_wraps_the_payload() {
        assert_eq!(osc52("hi"), "\u{1b}]52;c;aGk=\u{7}");
    }
}
