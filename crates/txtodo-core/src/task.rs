//! Views computed from a task's description on demand: projects, contexts, tags, id, ref slug.
//! Nothing here is stored; that is what keeps the CRDT faithful to the spec (design §3).

use crate::scanner::chunks;
use crate::tokenize::{WordKind, classify_word};
use crate::urls::DEFAULT_SCHEMES;
use crate::{Task, Ulid};

/// Longest slug the `ref:` tag accepts (plan §3.2.1).
pub const SLUG_MAX_LEN: usize = 64;

/// `[a-z0-9][a-z0-9._-]*`, 1–64 bytes, not `.` or `..`. No `/` is possible, so no path traversal.
pub fn is_valid_slug(s: &str) -> bool {
    let b = s.as_bytes();
    let head_ok = b
        .first()
        .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
    let tail_ok = b
        .iter()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'.' | b'_' | b'-'));
    let ok = head_ok && tail_ok && b.len() <= SLUG_MAX_LEN && s != "." && s != "..";
    debug_assert!(
        !ok || !s.contains('/'),
        "a valid slug never contains a slash"
    );
    ok
}

impl<'a> Task<'a> {
    /// Description words with their classification, using the default URL schemes.
    fn words(&self) -> impl Iterator<Item = (WordKind, &'a str)> {
        let d = self.description;
        chunks(d).filter(|c| !c.is_ws).map(move |c| {
            let w = &d[c.start..c.end];
            (classify_word(w, DEFAULT_SCHEMES), w)
        })
    }

    /// `+project` names without the sigil, in order, duplicates kept.
    pub fn projects(&self) -> impl Iterator<Item = &'a str> {
        self.words()
            .filter(|(k, _)| *k == WordKind::Project)
            .map(|(_, w)| &w[1..])
    }

    /// `@context` names without the sigil, in order, duplicates kept.
    pub fn contexts(&self) -> impl Iterator<Item = &'a str> {
        self.words()
            .filter(|(k, _)| *k == WordKind::Context)
            .map(|(_, w)| &w[1..])
    }

    /// `(key, value)` for every `key:value` word, including `id`, `ref`, `pri`. URLs are not tags.
    pub fn tags(&self) -> impl Iterator<Item = (&'a str, &'a str)> {
        self.words().filter_map(|(k, w)| match k {
            WordKind::Tag(colon) => Some((&w[..colon], &w[colon + 1..])),
            WordKind::IdTag => Some(("id", &w[3..])),
            _ => None,
        })
    }

    /// Value of the first tag named `key`.
    pub fn tag(&self, key: &str) -> Option<&'a str> {
        self.tags().find(|(k, _)| *k == key).map(|(_, v)| v)
    }

    /// Description words that are not a `+project`, `@context`, `key:value` tag, `id:`, or a URL,
    /// in order, duplicates kept. This is the "plain words" a `ref:` slug is generated from
    /// (plan §3.2 rule 4) — callers mint a slug from these rather than re-splitting the
    /// description and re-deriving the same classification.
    pub fn plain_words(&self) -> impl Iterator<Item = &'a str> {
        self.words()
            .filter(|(k, _)| *k == WordKind::Text)
            .map(|(_, w)| w)
    }

    /// Like [`Task::tag`] but with a custom URL scheme list (a word that is a URL is never a tag).
    pub fn tag_with_schemes(&self, key: &str, schemes: &[&str]) -> Option<&'a str> {
        let d = self.description;
        chunks(d)
            .filter(|c| !c.is_ws)
            .map(|c| &d[c.start..c.end])
            .find_map(|w| match classify_word(w, schemes) {
                WordKind::Tag(colon) if &w[..colon] == key => Some(&w[colon + 1..]),
                _ => None,
            })
    }

    /// The `id:` tag as a ULID; `None` when absent or malformed.
    pub fn id(&self) -> Option<Ulid> {
        self.tag("id").and_then(Ulid::parse)
    }

    /// The `ref:` slug, only if valid per plan §3.2.1; an invalid one is treated as no ref.
    pub fn ref_slug(&self) -> Option<&'a str> {
        self.tag("ref").filter(|s| is_valid_slug(s))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LineKind, Mode, parse_line};
    use alloc::vec::Vec;

    fn task(raw: &str) -> Task<'_> {
        match parse_line(raw, Mode::Lenient).unwrap().kind {
            LineKind::Task(t) => t,
            LineKind::Blank => panic!("blank"),
        }
    }

    #[test]
    fn views_follow_design_2_4() {
        let t = task("learn C++ +cpp mail bob@example.com see https://u@x.io/a +cpp @home");
        assert_eq!(t.projects().collect::<Vec<_>>(), ["cpp", "cpp"]);
        assert_eq!(t.contexts().collect::<Vec<_>>(), ["home"]);
        assert!(t.tags().next().is_none(), "the URL is not a tag");
        let t = task("买菜 +家务 @手机");
        assert_eq!(
            (t.projects().next(), t.contexts().next()),
            (Some("家务"), Some("手机"))
        );
    }

    #[test]
    fn plain_words_excludes_sigils_tags_and_urls() {
        let t = task("learn C++ +cpp see https://example.com/a ref:q4 id:x");
        assert_eq!(t.plain_words().collect::<Vec<_>>(), ["learn", "C++", "see"]);
        let t = task("买菜 +家务 @手机");
        assert_eq!(t.plain_words().collect::<Vec<_>>(), ["买菜"]);
    }

    #[test]
    fn tags_id_and_ref() {
        let t = task(
            "2026-09-11 Q4 +work ref:q4-roadmap id:01J9K3H5Z7Q8X2M4N6P8R0T2V4 due:2026-09-15 a:b:c note:",
        );
        assert_eq!(
            t.tags().collect::<Vec<_>>(),
            [
                ("ref", "q4-roadmap"),
                ("id", "01J9K3H5Z7Q8X2M4N6P8R0T2V4"),
                ("due", "2026-09-15"),
                ("a", "b:c")
            ]
        );
        assert_eq!(t.tag("due"), Some("2026-09-15"));
        assert_eq!(t.tag("missing"), None);
        assert_eq!(
            t.id().map(|u| alloc::format!("{u}")).as_deref(),
            Some("01J9K3H5Z7Q8X2M4N6P8R0T2V4")
        );
        assert_eq!(t.ref_slug(), Some("q4-roadmap"));
        assert_eq!(task("x id:short").id(), None);
        assert_eq!(
            task("t due:a due:b").tag("due"),
            Some("a"),
            "first match wins"
        );
    }

    #[test]
    fn slug_rules() {
        for good in ["q4-roadmap", "v1.2_beta-3", "a", "0", &"a".repeat(64)] {
            assert!(is_valid_slug(good), "{good}");
        }
        for bad in [
            ".",
            "..",
            "../escape",
            "/etc/passwd",
            "Q4-Roadmap",
            "-x",
            "_x",
            ".x",
            "",
            "a/b",
            "a b",
            &"a".repeat(65),
        ] {
            assert!(!is_valid_slug(bad), "{bad:?}");
        }
        for bad in [
            ".",
            "..",
            "../escape",
            "/etc/passwd",
            "Q4-Roadmap",
            "-x",
            "",
            &"a".repeat(65),
        ] {
            assert_eq!(
                task(&alloc::format!("t ref:{bad}")).ref_slug(),
                None,
                "{bad:?}"
            );
        }
    }
}
