//! Isolates the one call to `strsim` behind a single function (design §4.1, docs/questions.md
//! Q2), so it stays the crate's one swap point if the edit-distance implementation ever changes.
//! Ref: https://docs.rs/strsim/latest/strsim/fn.normalized_levenshtein.html
// Only `identity_fingerprint::cost` calls this today, and nothing calls that outside its own
// tests yet — wired in when `state.rs`/`reconcile_sidecar.rs` land (plan `floofy-swinging-brooks.md`).
#![allow(dead_code)]

/// Normalised edit distance between two descriptions: `0.0` identical, `1.0` maximally
/// different. `strsim::normalized_levenshtein` returns the similarity (`1.0` = identical), so
/// this is its complement.
pub fn description_distance(a: &str, b: &str) -> f64 {
    1.0 - strsim::normalized_levenshtein(a, b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_strings_have_zero_distance() {
        assert_eq!(description_distance("buy milk", "buy milk"), 0.0);
        assert_eq!(description_distance("", ""), 0.0);
    }

    #[test]
    fn a_full_rewrite_is_close_to_maximally_different() {
        assert!(description_distance("buy milk", "call the dentist") > 0.8);
    }

    #[test]
    fn distance_is_symmetric() {
        assert_eq!(
            description_distance("buy milk", "buy oat milk"),
            description_distance("buy oat milk", "buy milk")
        );
    }

    #[test]
    fn distance_is_between_zero_and_one() {
        for (a, b) in [("", "x"), ("abc", "xyz"), ("abc", "abcdef")] {
            let d = description_distance(a, b);
            assert!((0.0..=1.0).contains(&d), "{a:?} vs {b:?}: {d}");
        }
    }
}
