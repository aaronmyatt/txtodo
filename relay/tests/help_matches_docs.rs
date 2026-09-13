//! tasks/docs-relay-selfhost acceptance: "relay --help output matches every flag name
//! docs/relay.md mentions, failing on rename or removal" — the drift test that pins the doc's
//! flags table to the binary's actual flag list, in both directions.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::process::Command;

fn docs_relay_md() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("docs")
        .join("relay.md")
}

/// Drops every fenced code block (` ```...``` `) from `markdown`. The doc's real flag
/// references live in its flags table and prose (inline code); its "Build"/"Run" fenced
/// examples also happen to contain unrelated tool flags (`cargo build --release`) that would
/// otherwise register as bogus "relay flags" this test doesn't expect `--help` to mention.
fn strip_fenced_code_blocks(markdown: &str) -> String {
    markdown.split("```").step_by(2).collect()
}

/// Every `--flag-name` token in `text`: `--` followed by a lowercase letter, then any run of
/// lowercase letters and hyphens. Deliberately requires a letter right after `--` so a markdown
/// table's `|---|---|` separator row never counts as a flag.
fn flags_in(text: &str) -> BTreeSet<String> {
    let bytes = text.as_bytes();
    let mut flags = BTreeSet::new();
    let mut i = 0;
    while i < bytes.len() {
        match flag_at(bytes, i) {
            Some(end) => {
                flags.insert(text[i..end].to_owned());
                i = end;
            }
            None => i += 1,
        }
    }
    flags
}

/// If a `--flag` token starts at `bytes[i]`, returns its end index.
fn flag_at(bytes: &[u8], i: usize) -> Option<usize> {
    let is_dash = |k: usize| bytes.get(k) == Some(&b'-');
    if !is_dash(i) || !is_dash(i + 1) || !bytes.get(i + 2).is_some_and(u8::is_ascii_lowercase) {
        return None;
    }
    let mut j = i + 2;
    while bytes
        .get(j)
        .is_some_and(|b| b.is_ascii_lowercase() || *b == b'-')
    {
        j += 1;
    }
    Some(j)
}

#[test]
fn help_output_and_docs_relay_md_name_the_same_flags() {
    let output = Command::new(env!("CARGO_BIN_EXE_relay"))
        .arg("--help")
        .output()
        .expect("run relay --help");
    assert!(output.status.success(), "relay --help must exit 0");
    let help = String::from_utf8(output.stdout).expect("--help output is UTF-8");

    let doc = std::fs::read_to_string(docs_relay_md()).expect("read docs/relay.md");
    let doc = strip_fenced_code_blocks(&doc);

    let help_flags = flags_in(&help);
    let doc_flags = flags_in(&doc);

    assert!(
        !help_flags.is_empty(),
        "relay --help must actually list flags"
    );
    for flag in &help_flags {
        assert!(
            doc_flags.contains(flag),
            "docs/relay.md must mention {flag} (in --help)"
        );
    }
    for flag in &doc_flags {
        assert!(
            help_flags.contains(flag),
            "relay --help must mention {flag} (in docs/relay.md)"
        );
    }
}
