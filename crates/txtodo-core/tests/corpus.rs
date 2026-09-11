//! `just corpus`: every corpus file round-trips byte-for-byte, every line tokenises exactly as its
//! `.tokens.json` oracle says, lenient parsing is total, and strict fails only on the named leniencies.
//! Integration tests may use std and serde_json; the crate itself stays I/O- and serde-free.
// Integration tests are tests: clippy.toml allows unwrap/expect in #[test] fns but not in their helpers.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use txtodo_core::{Mode, Quirks, parse_file, parse_line, tokenize};

fn corpus_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus")
}

fn corpus_files() -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(corpus_dir())
        .expect("corpus dir")
        .map(|e| e.expect("entry").path())
        .filter(|p| p.extension().is_some_and(|e| e == "txt"))
        .collect();
    files.sort();
    assert!(
        files.len() >= 9,
        "expected the nine corpus files, found {}",
        files.len()
    );
    files
}

#[test]
fn every_corpus_file_round_trips_byte_for_byte() {
    for path in corpus_files() {
        let bytes = fs::read(&path).expect("read");
        assert_eq!(parse_file(&bytes).to_bytes(), bytes, "{}", path.display());
    }
}

#[test]
fn every_line_tokenizes_exactly_as_the_oracle() {
    for path in corpus_files() {
        let file = parse_file(&fs::read(&path).expect("read"));
        let oracle_path = path.with_extension("tokens.json");
        let oracle: Vec<Value> =
            serde_json::from_slice(&fs::read(&oracle_path).expect("oracle")).expect("json");
        assert_eq!(
            file.lines.len(),
            oracle.len(),
            "{}: line count",
            path.display()
        );
        for (i, (line, expected)) in file.lines.iter().zip(&oracle).enumerate() {
            let raw = line.raw().expect("corpus is UTF-8");
            assert_eq!(
                raw,
                expected["raw"].as_str().expect("raw"),
                "{}:{}",
                path.display(),
                i + 1
            );
            let got: Vec<Value> = tokenize(raw)
                .iter()
                .map(|s| serde_json::json!({ "kind": format!("{:?}", s.kind), "start": s.start, "end": s.end }))
                .collect();
            assert_eq!(
                Value::Array(got),
                expected["spans"],
                "{}:{} {raw:?}",
                path.display(),
                i + 1
            );
        }
    }
}

#[test]
fn lenient_is_total_and_strict_fails_only_on_named_leniencies() {
    let strict_only = Quirks::TABS | Quirks::TRAILING_WS | Quirks::LEADING_WS;
    let mut strict_failures = 0;
    for path in corpus_files() {
        for (i, line) in parse_file(&fs::read(&path).expect("read"))
            .lines
            .iter()
            .enumerate()
        {
            let raw = line.raw().expect("corpus is UTF-8");
            let lenient = parse_line(raw, Mode::Lenient)
                .unwrap_or_else(|e| panic!("{}:{} lenient failed: {e}", path.display(), i + 1));
            let strict = parse_line(raw, Mode::Strict);
            let expect_err = Quirks::ALL
                .iter()
                .any(|(q, _)| strict_only.has(*q) && lenient.quirks.has(*q));
            assert_eq!(
                strict.is_err(),
                expect_err,
                "{}:{} {raw:?} quirks={}",
                path.display(),
                i + 1,
                lenient.quirks
            );
            strict_failures += usize::from(strict.is_err());
        }
    }
    assert_eq!(
        strict_failures, 3,
        "lenient.txt lines 4,5,8 (tabs, trailing ws, runs) are the only strict failures"
    );
}
