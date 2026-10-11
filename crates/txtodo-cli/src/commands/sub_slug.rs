//! `sub <slug>[/<nested>…]` (task cli-sub-by-slug): names a sub-list by its `ref:` slugs instead of
//! a root line number that moves whenever a line above it completes. Each segment is looked up
//! among the `ref:` tags of the list before it, starting at the root list, and its line's ref dir
//! comes from the daemon (`RefDir`, never created here), so a nested path follows the tree the
//! way the daemon lays it out. An unknown slug is an error naming the closest one that exists.

use crate::CliError;
use crate::client::Daemon;
use txtodo_core::{LineKind, parse_file};
use txtodo_proto::v1 as pb;

/// The `ref:` slugs of `bytes`' task lines, each with its 1-based line number.
pub fn ref_lines(bytes: &[u8]) -> Vec<(u32, String)> {
    parse_file(bytes)
        .lines
        .iter()
        .enumerate()
        .filter_map(|(i, line)| match line.parse().map(|l| l.kind) {
            Some(LineKind::Task(t)) => t
                .ref_slug()
                .map(|s| (u32::try_from(i + 1).unwrap_or(u32::MAX), s.to_owned())),
            _ => None,
        })
        .collect()
}

/// Edit distance, for naming the closest slug. Wagner–Fischer:
/// <https://en.wikipedia.org/wiki/Wagner%E2%80%93Fischer_algorithm>
fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let here = row[j + 1];
            row[j + 1] = (prev + usize::from(ca != *cb))
                .min(row[j] + 1)
                .min(here + 1);
            prev = here;
        }
    }
    row[b.len()]
}

/// The line of `slug` among `refs`, or an error naming the closest slug there.
pub fn find(refs: &[(u32, String)], slug: &str, list: &str) -> Result<u32, CliError> {
    if let Some((n, _)) = refs.iter().find(|(_, s)| s == slug) {
        return Ok(*n);
    }
    let near = refs.iter().min_by_key(|(_, s)| distance(s, slug));
    Err(CliError::Message(match near {
        Some((_, s)) => format!("no line in {list} has ref:{slug}; the closest is ref:{s}"),
        None => format!("no line in {list} has ref:{slug}; it has no ref: lines"),
    }))
}

/// The sub-list file (`<ref dir>/todo.txt`) `path` names, walking from `root_list`.
pub fn resolve(daemon: &mut Daemon, root_list: &str, path: &str) -> Result<String, CliError> {
    let mut list = root_list.to_owned();
    for slug in path.split('/').filter(|s| !s.is_empty()) {
        let bytes = daemon.get(&list)?;
        let line = find(&ref_lines(&bytes), slug, &list)?;
        let task = pb::TaskRef {
            line_number: line,
            task_id: String::new(),
        };
        let info = daemon.ref_dir(&list, task, false)?;
        list = format!("{}/todo.txt", info.dir);
    }
    Ok(list)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ref_lines_are_found_with_their_numbers() {
        let refs = ref_lines(b"plain\nplan ref:trip\n\nbook ref:book-club\nx done ref:old\n");
        assert_eq!(
            refs,
            [
                (2, "trip".to_owned()),
                (4, "book-club".to_owned()),
                (5, "old".to_owned())
            ]
        );
    }

    #[test]
    fn an_unknown_slug_names_the_closest() {
        let refs = [(2, "trip".to_owned()), (4, "book-club".to_owned())];
        assert_eq!(find(&refs, "book-club", "todo.txt").unwrap_or(0), 4);
        let err = find(&refs, "bookclub", "todo.txt")
            .err()
            .map(|e| e.to_string())
            .unwrap_or_default();
        assert!(
            err.contains("ref:bookclub") && err.contains("ref:book-club"),
            "{err}"
        );
        assert!(find(&[], "x", "todo.txt").is_err());
    }
}
