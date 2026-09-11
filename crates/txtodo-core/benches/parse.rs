//! Plan M1 / §5 performance budget: parse 100 000 lines in ≤ 150 ms on the Linux CI runner
//! (`budgets.json.perf.parse100kMs`). `just bench-check` compares `parse_file_100k` against it.
//! criterion: https://bheisler.github.io/criterion.rs/book/
// Benches are not library code: unwrap is fine here.
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use criterion::{Criterion, Throughput, black_box, criterion_group, criterion_main};
use txtodo_core::{Mode, parse_file, parse_line, tokenize};

const LINES: usize = 100_000;

/// 100k lines cycled from the real corpus shapes, LF-terminated.
fn hundred_k() -> Vec<u8> {
    let corpus = [
        include_str!("../../../corpus/edge-cases.txt"),
        include_str!("../../../corpus/lenient.txt"),
        include_str!("../../../corpus/tags.txt"),
        include_str!("../../../corpus/refs.txt"),
        include_str!("../../../corpus/structure.txt"),
    ];
    let shapes: Vec<&str> = corpus
        .iter()
        .flat_map(|f| f.lines())
        .filter(|l| !l.is_empty())
        .collect();
    let mut out = String::with_capacity(LINES * 48);
    for i in 0..LINES {
        out.push_str(shapes[i % shapes.len()]);
        out.push('\n');
    }
    out.into_bytes()
}

fn benches(c: &mut Criterion) {
    let bytes = hundred_k();
    let text = String::from_utf8(bytes.clone()).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    let mut g = c.benchmark_group("parse");
    g.throughput(Throughput::Elements(LINES as u64));
    g.bench_function("parse_file_100k", |b| {
        b.iter(|| parse_file(black_box(&bytes)))
    });
    g.bench_function("parse_line_100k_lenient", |b| {
        b.iter(|| {
            for l in &lines {
                black_box(parse_line(l, Mode::Lenient).ok());
            }
        })
    });
    g.bench_function("tokenize_100k", |b| {
        b.iter(|| {
            for l in &lines {
                black_box(tokenize(l));
            }
        })
    });
    g.finish();
}

criterion_group!(group, benches);
criterion_main!(group);
