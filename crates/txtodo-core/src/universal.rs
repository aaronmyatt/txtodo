//! The Universal screen's grouping (task `tui-revamp/shared-core`): due-date buckets and badges,
//! and rows grouped by priority, due, project, context or workspace, in the c2 mockup's order
//! (`apps/desktop/design-mockups/c2/universal.js`, `shared/todotxt.js` `dueLabel`). The daemon
//! sends a row's raw `due:` value; each client buckets it against its own local today (ADR 0011),
//! passed in here, since core has no clock.
//! Ref: <https://howardhinnant.github.io/date_algorithms.html#days_from_civil>

use alloc::string::String;
use alloc::vec::Vec;
use alloc::{format, vec};
use core::cmp::Ordering;

use crate::Date;

/// Days since 1970-01-01 of a valid date (Howard Hinnant's `days_from_civil`).
fn days_from_civil(d: Date) -> i64 {
    let (y, m, day) = (
        i64::from(d.year()),
        i64::from(d.month()),
        i64::from(d.day()),
    );
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Whole days from `from` to `to` (both `YYYY-MM-DD`), or `None` when either is not a real date.
pub fn days_between(from: &str, to: &str) -> Option<i64> {
    let (from, to) = (Date::parse(from)?, Date::parse(to)?);
    Some(days_from_civil(to) - days_from_civil(from))
}

/// Where a due date falls, in display order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum DueBucket {
    /// Before today.
    Overdue,
    /// Today.
    Today,
    /// Tomorrow through seven days out.
    ThisWeek,
    /// Further out.
    Later,
    /// No `due:`, or not a real date.
    NoDate,
}

impl DueBucket {
    /// The group heading.
    pub fn label(self) -> &'static str {
        match self {
            DueBucket::Overdue => "Overdue",
            DueBucket::Today => "Today",
            DueBucket::ThisWeek => "This week",
            DueBucket::Later => "Later",
            DueBucket::NoDate => "No date",
        }
    }
}

/// The bucket of `due` against `today`.
pub fn due_bucket(due: Option<&str>, today: &str) -> DueBucket {
    match due.and_then(|d| days_between(today, d)) {
        None => DueBucket::NoDate,
        Some(d) if d < 0 => DueBucket::Overdue,
        Some(0) => DueBucket::Today,
        Some(d) if d <= 7 => DueBucket::ThisWeek,
        Some(_) => DueBucket::Later,
    }
}

/// A row's due badge: `overdue 2d`, `today`, `tomorrow`, `in 4d`, or `Oct 15`, with its days
/// from today (negative when overdue). `None` for no date.
pub fn due_label(due: Option<&str>, today: &str) -> Option<(String, i64)> {
    let due = due?;
    let d = days_between(today, due)?;
    let text = match d {
        d if d < 0 => format!("overdue {}d", -d),
        0 => String::from("today"),
        1 => String::from("tomorrow"),
        d if d <= 7 => format!("in {d}d"),
        _ => {
            let date = Date::parse(due)?;
            const MONTHS: [&str; 12] = [
                "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
            ];
            format!("{} {}", MONTHS[usize::from(date.month() - 1)], date.day())
        }
    };
    Some((text, d))
}

/// What the Universal screen groups by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupBy {
    /// `(A)`… then `No priority` (the default).
    Priority,
    /// Overdue, Today, This week, Later, No date.
    Due,
    /// The first `+project`, then `No project`.
    Project,
    /// The first `@context`, then `No context`.
    Context,
    /// The workspace, in the order the caller lists them.
    Workspace,
}

/// The facts about one row that grouping reads. Names are bare (no `+`/`@`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RowFacts<'a> {
    /// A completed line.
    pub done: bool,
    /// Its priority letter.
    pub priority: Option<char>,
    /// Its raw `due:` value.
    pub due: Option<&'a str>,
    /// Its first project.
    pub project: Option<&'a str>,
    /// Its first context.
    pub context: Option<&'a str>,
    /// Its workspace's name.
    pub workspace: &'a str,
}

/// The group heading `row` goes under.
pub fn group_name(row: &RowFacts<'_>, by: GroupBy, today: &str) -> String {
    match by {
        GroupBy::Priority => row
            .priority
            .map_or_else(|| String::from("No priority"), |p| format!("({p})")),
        GroupBy::Due => String::from(due_bucket(row.due, today).label()),
        GroupBy::Project => String::from(row.project.unwrap_or("No project")),
        GroupBy::Context => String::from(row.context.unwrap_or("No context")),
        GroupBy::Workspace => String::from(row.workspace),
    }
}

/// Where a group heading sorts, before its name: the mockup's `groupRank`.
fn group_rank(name: &str, by: GroupBy, workspaces: &[&str]) -> i64 {
    let unknown = i64::MAX / 2;
    match by {
        GroupBy::Due => [
            DueBucket::Overdue,
            DueBucket::Today,
            DueBucket::ThisWeek,
            DueBucket::Later,
            DueBucket::NoDate,
        ]
        .iter()
        .position(|b| b.label() == name)
        .map_or(unknown, |i| i as i64),
        GroupBy::Workspace => workspaces
            .iter()
            .position(|w| *w == name)
            .map_or(unknown, |i| i as i64),
        GroupBy::Priority => match name.as_bytes() {
            [b'(', p, ..] => i64::from(*p),
            _ => 999,
        },
        GroupBy::Project | GroupBy::Context => {
            if name.starts_with("No ") {
                9999
            } else {
                0
            }
        }
    }
}

/// A row's place inside its group: open before done, then by due date (none last), then by
/// priority (none last). The mockup's `rank`.
fn row_rank(row: &RowFacts<'_>, today: &str) -> i64 {
    let done = if row.done { 1_000_000 } else { 0 };
    let due = row
        .due
        .and_then(|d| days_between(today, d))
        .map_or(5000, |d| d + 1000);
    let pri = row.priority.map_or(9, |p| i64::from(u32::from(p)) - 64);
    done + due * 10 + pri
}

/// Case-folded, then exact: a stand-in for the mockup's `localeCompare`.
fn name_order(a: &str, b: &str) -> Ordering {
    a.to_lowercase()
        .cmp(&b.to_lowercase())
        .then_with(|| a.cmp(b))
}

/// `rows` grouped by `by`: each heading with the indices of its rows, headings and rows in display
/// order. `workspaces` orders [`GroupBy::Workspace`].
pub fn group(
    rows: &[RowFacts<'_>],
    by: GroupBy,
    today: &str,
    workspaces: &[&str],
) -> Vec<(String, Vec<usize>)> {
    let mut groups: Vec<(String, Vec<usize>)> = Vec::new();
    for (i, row) in rows.iter().enumerate() {
        let name = group_name(row, by, today);
        match groups.iter_mut().find(|(n, _)| *n == name) {
            Some((_, members)) => members.push(i),
            None => groups.push((name, vec![i])),
        }
    }
    groups.sort_by(|(a, _), (b, _)| {
        group_rank(a, by, workspaces)
            .cmp(&group_rank(b, by, workspaces))
            .then_with(|| name_order(a, b))
    });
    for (_, members) in &mut groups {
        members.sort_by_key(|&i| row_rank(&rows[i], today));
    }
    debug_assert_eq!(
        groups.iter().map(|(_, m)| m.len()).sum::<usize>(),
        rows.len()
    );
    groups
}

#[cfg(test)]
mod tests {
    use super::*;

    const TODAY: &str = "2026-09-25";

    #[test]
    fn days_between_crosses_months_years_and_leap_days() {
        assert_eq!(days_between(TODAY, TODAY), Some(0));
        assert_eq!(days_between("2026-09-25", "2026-10-01"), Some(6));
        assert_eq!(days_between("2026-12-31", "2027-01-01"), Some(1));
        assert_eq!(days_between("2028-02-28", "2028-03-01"), Some(2));
        assert_eq!(days_between("2026-09-25", "2026-09-20"), Some(-5));
        assert_eq!(days_between("2026-09-25", "2026-02-30"), None);
        assert_eq!(days_between("tomorrow", TODAY), None);
    }

    #[test]
    fn buckets_follow_the_mockup() {
        let b = |d: &str| due_bucket(Some(d), TODAY);
        assert_eq!(b("2026-09-23"), DueBucket::Overdue);
        assert_eq!(b(TODAY), DueBucket::Today);
        assert_eq!(b("2026-09-26"), DueBucket::ThisWeek);
        assert_eq!(b("2026-10-02"), DueBucket::ThisWeek);
        assert_eq!(b("2026-10-03"), DueBucket::Later);
        assert_eq!(b("soon"), DueBucket::NoDate);
        assert_eq!(due_bucket(None, TODAY), DueBucket::NoDate);
    }

    #[test]
    fn badges_follow_the_mockup() {
        let l = |d: &str| due_label(Some(d), TODAY).map(|(t, _)| t);
        assert_eq!(l("2026-09-23").as_deref(), Some("overdue 2d"));
        assert_eq!(l(TODAY).as_deref(), Some("today"));
        assert_eq!(l("2026-09-26").as_deref(), Some("tomorrow"));
        assert_eq!(l("2026-09-29").as_deref(), Some("in 4d"));
        assert_eq!(l("2026-10-15").as_deref(), Some("Oct 15"));
        assert_eq!(l("nope"), None);
    }

    fn row<'a>(pri: Option<char>, due: Option<&'a str>, ws: &'a str) -> RowFacts<'a> {
        RowFacts {
            done: false,
            priority: pri,
            due,
            project: None,
            context: None,
            workspace: ws,
        }
    }

    #[test]
    fn groups_come_in_the_mockups_order_and_rows_by_rank() {
        let rows = [
            row(None, None, "home"),
            row(Some('B'), Some("2026-09-24"), "work"),
            row(Some('A'), None, "home"),
            row(Some('B'), Some("2026-09-30"), "home"),
            RowFacts {
                done: true,
                ..row(Some('B'), Some("2026-09-20"), "work")
            },
        ];
        let by_pri = group(&rows, GroupBy::Priority, TODAY, &["home", "work"]);
        let names: Vec<&str> = by_pri.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["(A)", "(B)", "No priority"]);
        assert_eq!(by_pri[1].1, [1, 3, 4], "overdue first, the done one last");
        let by_due = group(&rows, GroupBy::Due, TODAY, &[]);
        let names: Vec<&str> = by_due.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["Overdue", "This week", "No date"]);
        let by_ws = group(&rows, GroupBy::Workspace, TODAY, &["work", "home"]);
        assert_eq!(by_ws[0].0, "work");
        let by_project = group(&rows, GroupBy::Project, TODAY, &[]);
        assert_eq!(by_project.len(), 1);
        assert_eq!(by_project[0].0, "No project");
    }
}
