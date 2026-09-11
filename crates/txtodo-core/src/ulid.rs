//! ULID parsing and display for the `id:` tag. Spec: <https://github.com/ulid/spec>. No external crate:
//! core needs decode and encode only.

use core::fmt;

/// A 128-bit ULID: 48-bit timestamp + 80-bit randomness, shown as 26 Crockford base32 characters.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Ulid(u128);

/// Crockford base32 alphabet: digits and uppercase letters without I, L, O, U.
const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
/// Text length of a ULID.
pub const ULID_LEN: usize = 26;

impl Ulid {
    /// Wraps raw bits.
    pub const fn from_u128(bits: u128) -> Ulid {
        Ulid(bits)
    }
    /// The raw bits.
    pub const fn to_u128(self) -> u128 {
        self.0
    }
    /// Parses 26 Crockford characters (uppercase only, as written by ULID generators). The first character
    /// must be `0`–`7` so the value fits in 128 bits. Anything else is `None`, never an error.
    pub fn parse(s: &str) -> Option<Ulid> {
        let b = s.as_bytes();
        if b.len() != ULID_LEN || b[0] > b'7' {
            return None;
        }
        let mut bits: u128 = 0;
        for &c in b {
            let v = decode_char(c)?;
            debug_assert!(v < 32, "decode_char yields a 5-bit value");
            bits = (bits << 5) | u128::from(v);
        }
        Some(Ulid(bits))
    }
}

/// Value of one Crockford character, or `None`.
fn decode_char(c: u8) -> Option<u8> {
    let idx = ALPHABET.iter().position(|&a| a == c)?;
    debug_assert!(idx < 32, "alphabet has 32 symbols");
    Some(idx as u8)
}

impl fmt::Display for Ulid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut out = [b'0'; ULID_LEN];
        let mut bits = self.0;
        for slot in out.iter_mut().rev() {
            *slot = ALPHABET[(bits & 31) as usize];
            bits >>= 5;
        }
        debug_assert!(bits == 0, "128 bits fit in 26 five-bit chars with room to spare");
        // The alphabet is ASCII, so this cannot fail; fall through to an empty string rather than panic.
        f.write_str(core::str::from_utf8(&out).unwrap_or(""))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_the_spec_example() {
        let text = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
        let u = Ulid::parse(text).unwrap();
        assert_eq!(alloc::format!("{u}"), text);
        assert_eq!(u.to_u128() >> 80, 1_469_922_850_259, "timestamp: hand-decoded 01ARZ3NDEK");
        assert_eq!(alloc::format!("{}", Ulid::from_u128(1 << 80)), "00000000010000000000000000", "bit 80 is the 17th char from the right");
    }

    #[test]
    fn rejects_bad_length_overflow_and_excluded_letters() {
        assert_eq!(Ulid::parse("01ARZ3NDEKTSV4RRFFQ69G5FA"), None, "25 chars");
        assert_eq!(Ulid::parse("8ARZ3NDEKTSV4RRFFQ69G5FAVX"), None, "first char > 7");
        assert_eq!(Ulid::parse("01ARZ3NDEKTSV4RRFFQ69G5FAI"), None, "I is excluded");
        assert_eq!(Ulid::parse("01arz3ndektsv4rrffq69g5fav"), None, "lowercase");
    }

    #[test]
    fn zero_displays_as_all_zeros() {
        assert_eq!(alloc::format!("{}", Ulid::from_u128(0)), "00000000000000000000000000");
    }
}
