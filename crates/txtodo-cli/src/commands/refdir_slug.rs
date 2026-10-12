//! `sub <slug>[/<sub>] COMMAND...` (tasks/cli-sub-by-slug): a sub-list named by its slug path, which
//! does not move when a line above its parent completes or is added, unlike the root line number
//! `sub ITEM#` takes. Resolved against the daemon's own file list, so a custom refs folder and
//! nested refs (`a/b` is `<refs>/a/b/todo.txt`) need no layout lookup here.

use crate::CliError;
use crate::client::Daemon;

/// The workspace-relative `todo.txt` whose directory path ends in `slug`; the shortest if several
/// do. An unknown slug is an error naming the nearest sub-list.
pub fn list_of_slug(daemon: &mut Daemon, slug: &str) -> Result<String, CliError> {
    let slug = slug.trim_matches('/');
    let suffix = format!("/{slug}/todo.txt");
    let lists: Vec<String> = daemon
        .list_files()?
        .into_iter()
        .map(|f| f.path)
        .filter(|p| p.ends_with("/todo.txt"))
        .collect();
    if let Some(found) = lists
        .iter()
        .filter(|p| p.ends_with(&suffix))
        .min_by_key(|p| p.len())
    {
        return Ok(found.clone());
    }
    let nearest = lists
        .iter()
        .map(|p| tail(dir_of(p), slug.split('/').count()))
        .min_by_key(|candidate| distance(candidate, slug));
    Err(CliError::Message(match nearest {
        Some(n) => format!("no sub-list {slug:?}; the nearest is {n:?}"),
        None => format!("no sub-list {slug:?}; this workspace has none"),
    }))
}

/// `tasks/a/b/todo.txt` → `tasks/a/b`.
fn dir_of(list: &str) -> &str {
    list.strip_suffix("/todo.txt").unwrap_or(list)
}

/// The last `n` `/`-separated parts of `dir`, the shape a slug path of `n` parts is compared to.
fn tail(dir: &str, n: usize) -> &str {
    let mut cut = dir.len();
    for _ in 0..n {
        match dir[..cut].rfind('/') {
            Some(i) => cut = i,
            None => return dir,
        }
    }
    &dir[cut + 1..]
}

/// Levenshtein distance by chars; slugs are short.
/// Ref: <https://en.wikipedia.org/wiki/Levenshtein_distance#Iterative_with_two_matrix_rows>
fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut row = vec![i + 1; b.len() + 1];
        for (j, cb) in b.iter().enumerate() {
            let swap = prev[j] + usize::from(ca != *cb);
            row[j + 1] = swap.min(prev[j + 1] + 1).min(row[j] + 1);
        }
        prev = row;
    }
    prev[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_slug_path_is_compared_to_as_many_trailing_parts() {
        assert_eq!(tail("tasks/trip/flights", 1), "flights");
        assert_eq!(tail("tasks/trip/flights", 2), "trip/flights");
        assert_eq!(tail("trip", 2), "trip");
        assert_eq!(dir_of("tasks/trip/todo.txt"), "tasks/trip");
    }

    #[test]
    fn distance_counts_single_char_edits() {
        assert_eq!(distance("tirp", "trip"), 2);
        assert_eq!(distance("trip", "trip"), 0);
        assert_eq!(distance("", "ab"), 2);
    }
}
