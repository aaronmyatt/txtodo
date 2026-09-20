//! A document as `GetFile` answers it: the text plus the daemon's task id for every line (task
//! sidecar-task-ids). Under Sidecar identity (ADR 0019) a line carries no `id:` tag, so the text
//! alone cannot say which task a line is; `FileContents.task_ids` can. When the daemon sent ids
//! they have the last word. When it sent none (a daemon older than the field) every lookup falls
//! back to the first `id:` word in the text, which is what this crate did before.

use crate::backend::TaskRow;
use crate::parse;

/// One document: its text and one task id per line.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileDoc {
    /// The file as UTF-8 text (lossy; a byte-exact round trip is the CLI's job).
    pub text: String,
    /// One entry per line of `text`, `""` for a blank line; empty when the daemon sent none.
    pub task_ids: Vec<String>,
}

impl FileDoc {
    /// A document with no ids from the daemon: every lookup reads the `id:` tag in the text.
    #[cfg(test)]
    pub fn from_text(text: impl Into<String>) -> FileDoc {
        FileDoc {
            text: text.into(),
            task_ids: Vec::new(),
        }
    }

    /// The daemon's id for 1-based `line`; `None` for a blank line or a line past the end.
    fn daemon_id(&self, line: u32) -> Option<&str> {
        let i = (line as usize).checked_sub(1)?;
        self.task_ids
            .get(i)
            .map(String::as_str)
            .filter(|id| !id.is_empty())
    }

    /// `raw` (line `line` of this document) as a [`TaskRow`] whose `id` is the daemon's. A leftover
    /// `id:` word in a Sidecar line is plain text, not the identity: it moves to `kv` (last).
    pub fn row(&self, line: u32, raw: &str) -> TaskRow {
        let mut row = parse::parse_row(line, raw);
        if self.task_ids.is_empty() {
            return row;
        }
        let tag = row.id.take();
        row.id = self.daemon_id(line).map(str::to_owned);
        if let Some(tag) = tag.filter(|t| row.id.as_deref() != Some(t.as_str())) {
            row.kv.push(("id".to_owned(), tag));
        }
        row
    }

    /// Every line as a row, blank lines included (callers drop the ones they do not want).
    pub fn rows(&self) -> Vec<TaskRow> {
        parse::lines(&self.text)
            .into_iter()
            .map(|(n, raw)| self.row(n, raw))
            .collect()
    }

    /// The line holding task `id`, first match: `(1-based line, raw text)`.
    pub fn find_by_id(&self, id: &str) -> Option<(u32, &str)> {
        if self.task_ids.is_empty() {
            return parse::find_by_id(&self.text, id);
        }
        // A blank line's entry is "": an empty `id` must not match it.
        let i = self
            .task_ids
            .iter()
            .position(|t| !id.is_empty() && t == id)?;
        let line = u32::try_from(i).ok()? + 1;
        parse::lines(&self.text)
            .into_iter()
            .find(|(n, _)| *n == line)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "01M2T868JD32M84JQQ2ABASXW4";
    const B: &str = "01M2T868JD32M84JQQ2ABASXW5";

    fn sidecar() -> FileDoc {
        FileDoc {
            text: "one\n\ntwo +p\n".to_owned(),
            task_ids: vec![A.to_owned(), String::new(), B.to_owned()],
        }
    }

    #[test]
    fn a_sidecar_line_with_no_tag_gets_the_daemons_id() {
        let rows = sidecar().rows();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].id.as_deref(), Some(A));
        assert_eq!(rows[1].id, None, "a blank line has no id");
        assert_eq!(rows[2].id.as_deref(), Some(B));
        assert_eq!(rows[2].projects, vec!["p".to_owned()]);
    }

    #[test]
    fn find_by_id_uses_the_daemons_ids() {
        let doc = sidecar();
        assert_eq!(doc.find_by_id(B), Some((3, "two +p")));
        assert_eq!(doc.find_by_id("nope"), None);
        assert_eq!(
            doc.find_by_id(""),
            None,
            "the blank line's empty entry is not an id"
        );
    }

    #[test]
    fn a_leftover_id_word_is_not_the_identity_when_the_daemon_sent_ids() {
        let doc = FileDoc {
            text: format!("one id:{B}\n"),
            task_ids: vec![A.to_owned()],
        };
        let row = doc.row(1, doc.text.trim_end());
        assert_eq!(row.id.as_deref(), Some(A));
        assert_eq!(row.kv, vec![("id".to_owned(), B.to_owned())]);
        assert_eq!(doc.find_by_id(B), None);
    }

    #[test]
    fn with_no_ids_from_the_daemon_the_tag_in_the_text_is_the_id() {
        let doc = FileDoc::from_text(format!("one id:{A}\ntwo\n"));
        assert_eq!(doc.rows()[0].id.as_deref(), Some(A));
        assert_eq!(doc.rows()[1].id, None);
        assert_eq!(doc.find_by_id(A).map(|(n, _)| n), Some(1));
    }
}
