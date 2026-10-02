//! What [`OpKind::RemoveTag`](crate::OpKind::RemoveTag) does to a description (ADR 0036). One
//! implementation, so every device, the daemon's state and its Loro mirror alike, removes the same
//! chars. It follows core's `Edit::remove_tag`: words split on spaces and tabs, the first word that
//! is `key:` plus a non-empty value goes, with the space before it, else the space after it.
//! Unlike core it reads no URL schemes: `key` is never one (`pri`, the only key sent today).
//! todo.txt tags: <https://github.com/todotxt/todo.txt#additional-file-format-definitions>

/// Whether `key` can name a tag: non-empty, no space, tab or `:`. A `RemoveTag` with any other key
/// is refused.
pub fn valid_tag_key(key: &str) -> bool {
    !key.is_empty() && !key.bytes().any(|b| b == b' ' || b == b'\t' || b == b':')
}

/// `description` with its first `key:value` word removed, and one space next to it. Unchanged when
/// there is none, or when `key` is not [`valid_tag_key`].
pub fn remove_tag(description: &str, key: &str) -> String {
    let Some((start, end)) = find_tag(description, key) else {
        return description.to_owned();
    };
    let bytes = description.as_bytes();
    let (from, to) = if start > 0 && bytes[start - 1] == b' ' {
        (start - 1, end)
    } else if bytes.get(end) == Some(&b' ') {
        (start, end + 1)
    } else {
        (start, end)
    };
    debug_assert!(from <= start && to >= end, "removal covers the word");
    format!("{}{}", &description[..from], &description[to..])
}

/// Byte range of the first `key:value` word, `None` without one.
fn find_tag(description: &str, key: &str) -> Option<(usize, usize)> {
    if !valid_tag_key(key) {
        return None;
    }
    let mut start = 0;
    for word in description.split([' ', '\t']) {
        let end = start + word.len();
        let value = word.strip_prefix(key).and_then(|w| w.strip_prefix(':'));
        if value.is_some_and(|v| !v.is_empty()) {
            return Some((start, end));
        }
        // The separator is one ASCII byte.
        start = end + 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_tag_goes_with_the_space_before_it_else_after_it() {
        assert_eq!(remove_tag("call mum pri:B", "pri"), "call mum");
        assert_eq!(remove_tag("pri:B call mum", "pri"), "call mum");
        assert_eq!(remove_tag("call pri:B mum pri:C", "pri"), "call mum pri:C");
        assert_eq!(remove_tag("pri:B", "pri"), "");
        assert_eq!(remove_tag("a\tpri:B\tb", "pri"), "a\t\tb");
    }

    #[test]
    fn no_tag_a_bare_key_or_a_bad_key_changes_nothing() {
        assert_eq!(remove_tag("call mum", "pri"), "call mum");
        assert_eq!(remove_tag("call pri: mum", "pri"), "call pri: mum");
        assert_eq!(remove_tag("call xpri:B", "pri"), "call xpri:B");
        assert_eq!(remove_tag("call pri:B", "pri:"), "call pri:B");
        assert_eq!(remove_tag("call pri:B", ""), "call pri:B");
        assert!(valid_tag_key("pri") && !valid_tag_key("a b") && !valid_tag_key("a:b"));
    }

    #[test]
    fn multibyte_text_around_the_tag_stays_whole() {
        assert_eq!(remove_tag("héllo pri:B wörld", "pri"), "héllo wörld");
    }
}
