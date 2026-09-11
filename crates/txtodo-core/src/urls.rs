//! URL detection, which runs before tag detection (design §2.4): `see https://example.com/x` is not a tag.

/// Schemes recognised by default. Configurable per call; order is irrelevant.
/// Registry: <https://www.iana.org/assignments/uri-schemes>
pub const DEFAULT_SCHEMES: &[&str] = &[
    "http", "https", "mailto", "ftp", "ssh", "file", "tel", "sms",
];

/// True when `word` is `<scheme>:<at least one byte>` and `scheme` (ASCII case-insensitive) is in `schemes`.
/// `note:` is never a URL (nothing after the colon); `a:b:c` is not unless `a` is a listed scheme.
pub fn is_url(word: &str, schemes: &[&str]) -> bool {
    let Some(colon) = word.find(':') else {
        return false;
    };
    let (scheme, rest) = word.split_at(colon);
    debug_assert!(rest.starts_with(':'), "split at the colon");
    debug_assert!(
        !scheme.contains(':'),
        "scheme is the part before the first colon"
    );
    rest.len() > 1 && schemes.iter().any(|s| s.eq_ignore_ascii_case(scheme))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_default_scheme_is_recognised_case_insensitively() {
        for s in DEFAULT_SCHEMES {
            assert!(is_url(&alloc::format!("{s}:x"), DEFAULT_SCHEMES), "{s}");
            assert!(
                is_url(
                    &alloc::format!("{}://x", s.to_ascii_uppercase()),
                    DEFAULT_SCHEMES
                ),
                "{s} upper"
            );
        }
        assert!(is_url("mailto:x", DEFAULT_SCHEMES));
        assert!(is_url("tel:+1", DEFAULT_SCHEMES));
        assert!(is_url("https://user@example.com/x", DEFAULT_SCHEMES));
    }

    #[test]
    fn non_urls() {
        assert!(!is_url("note:", DEFAULT_SCHEMES), "empty value");
        assert!(
            !is_url("https:", DEFAULT_SCHEMES),
            "scheme with nothing after"
        );
        assert!(!is_url("a:b:c", DEFAULT_SCHEMES), "unknown scheme");
        assert!(!is_url("color:blue", DEFAULT_SCHEMES), "ordinary tag");
        assert!(!is_url("https://x", &[]), "empty scheme list");
        assert!(!is_url("plain", DEFAULT_SCHEMES));
    }
}
