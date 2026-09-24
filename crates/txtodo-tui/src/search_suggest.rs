//! Search suggestions (task `tui-revamp/tui-tasks`, c2 `c2-prompt.html:392-404`): while the search
//! field has the keyboard, a panel offers the list's most used contexts and projects, `(A)`–`(C)`,
//! `is:open`, `is:done`, `due:` and this session's recent queries. A click toggles a term in the
//! query; Tab completes the word being typed to the first term that extends it, or adds the first
//! term not in the query yet.
//! Ref: <https://docs.rs/txtodo-core> (`tokenize`, the same token kinds the rows colour)

use std::collections::BTreeMap;

use txtodo_core::{TokenKind, tokenize};

use crate::state::AppState;

/// Contexts and projects shown, most used first (the c2 panel's six and five).
const CONTEXTS: usize = 6;
const PROJECTS: usize = 5;
/// The fixed terms after them.
const FIXED: [&str; 6] = ["(A)", "(B)", "(C)", "is:open", "is:done", "due:"];
/// Recent queries kept.
pub const RECENTS: usize = 3;

/// The terms offered for the open list, in panel order.
pub fn suggestions(state: &AppState) -> Vec<String> {
    let mut counts: [BTreeMap<String, usize>; 2] = Default::default();
    for line in &state.lines {
        for span in tokenize(&line.raw) {
            let slot = match span.kind {
                TokenKind::Context => 0,
                TokenKind::Project => 1,
                _ => continue,
            };
            *counts[slot]
                .entry(line.raw[span.start..span.end].to_owned())
                .or_default() += 1;
        }
    }
    let top = |map: &BTreeMap<String, usize>, n: usize| {
        let mut terms: Vec<(&String, &usize)> = map.iter().collect();
        terms.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
        terms
            .into_iter()
            .take(n)
            .map(|(t, _)| t.clone())
            .collect::<Vec<_>>()
    };
    let mut out = top(&counts[0], CONTEXTS);
    out.extend(top(&counts[1], PROJECTS));
    out.extend(FIXED.iter().map(|t| (*t).to_owned()));
    out
}

/// Whether `query` has `term` as a word, ignoring case.
pub fn has_term(query: &str, term: &str) -> bool {
    query
        .split_whitespace()
        .any(|w| w.eq_ignore_ascii_case(term))
}

/// `query` with `term` added, or taken out if it is there.
pub fn toggle(query: &str, term: &str) -> String {
    let mut words: Vec<&str> = query.split_whitespace().collect();
    match words.iter().position(|w| w.eq_ignore_ascii_case(term)) {
        Some(i) => {
            words.remove(i);
        }
        None => words.push(term),
    }
    words.join(" ")
}

/// Tab: the word being typed completed to the first suggestion that extends it, or, with no word
/// being typed, the first suggestion not in the query added. `None` when neither applies.
pub fn complete(query: &str, suggestions: &[String]) -> Option<String> {
    let typing = !query.is_empty() && !query.ends_with(char::is_whitespace);
    if typing {
        let start = query.rfind(char::is_whitespace).map_or(0, |i| i + 1);
        let partial = query[start..].to_lowercase();
        let pick = suggestions
            .iter()
            .find(|s| s.len() > partial.len() && s.to_lowercase().starts_with(&partial))?;
        return Some(format!("{}{pick}", &query[..start]));
    }
    let pick = suggestions.iter().find(|s| !has_term(query, s))?;
    Some(toggle(query, pick))
}

/// Remembers `query` as the newest recent one (no repeats, at most [`RECENTS`]).
pub fn remember(recents: &mut Vec<String>, query: &str) {
    let query = query.trim();
    if query.is_empty() {
        return;
    }
    recents.retain(|r| r != query);
    recents.insert(0, query.to_owned());
    recents.truncate(RECENTS);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lists_contexts_and_projects_come_first_then_the_fixed_terms() {
        let state =
            AppState::from_document("todo.txt", "a @desk +work\nb @desk\nc @home +work +home");
        let s = suggestions(&state);
        assert_eq!(&s[..4], ["@desk", "@home", "+work", "+home"]);
        assert_eq!(&s[4..], FIXED);
    }

    #[test]
    fn toggle_adds_or_drops_a_term_ignoring_case() {
        assert_eq!(toggle("milk", "@desk"), "milk @desk");
        assert_eq!(toggle("milk @DESK", "@desk"), "milk");
    }

    #[test]
    fn tab_completes_the_word_being_typed_or_adds_the_next_term() {
        let s: Vec<String> = ["@desk", "@home", "is:open"].map(str::to_owned).to_vec();
        assert_eq!(complete("milk @h", &s).as_deref(), Some("milk @home"));
        assert_eq!(complete("is:", &s).as_deref(), Some("is:open"));
        assert_eq!(complete("", &s).as_deref(), Some("@desk"));
        assert_eq!(complete("@desk ", &s).as_deref(), Some("@desk @home"));
        assert_eq!(complete("zz", &s), None);
    }

    #[test]
    fn recents_keep_the_newest_three_without_repeats() {
        let mut recents = Vec::new();
        for q in ["a", "b", "a", "c", "d", " "] {
            remember(&mut recents, q);
        }
        assert_eq!(recents, ["d", "c", "a"]);
    }
}
