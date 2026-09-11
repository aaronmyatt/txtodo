//! Perf budget (plan §5, budgets.json.latencyMs): reconcile one external line edit in a 10k-line
//! file ≤ 20 ms on the Linux CI runner. `reconcile_10k_one_edit` is the budgeted bench (the pure
//! reconciler); the others are informational (parse has its own budget in core) or attribution.
//! Ref: https://bheisler.github.io/criterion.rs/book/
// criterion_group! expands to an undocumented `fn benches`; a bench is not library API.
#![allow(missing_docs)]

use criterion::{Criterion, black_box, criterion_group, criterion_main};
use txtodo_core::{File, parse_file};
use txtodo_daemon::reconcile::reconcile;
use txtodo_daemon::state::{id_of, task_id};
use txtodo_model::FilePath;

const LINES: usize = 10_000;

/// Ten thousand task lines shaped like the corpus, every one with an id.
fn fixture() -> Vec<u8> {
    let mut out = String::with_capacity(LINES * 64);
    for i in 0..LINES {
        let pri = ['A', 'B', 'C', 'D'][i % 4];
        out.push_str(&format!(
            "({pri}) 2026-09-11 task number {i} +project{} @ctx{} due:2026-10-01 id:{}\n",
            i % 17,
            i % 5,
            task_id(0x0100_0000 + i as u128).ulid()
        ));
    }
    out.into_bytes()
}

fn edit_one_line(bytes: &[u8]) -> Vec<u8> {
    let text = String::from_utf8_lossy(bytes).replacen(
        "task number 5000 ",
        "task number 5000 (edited) ",
        1,
    );
    text.into_bytes()
}

fn bench(c: &mut Criterion) {
    let old_bytes = fixture();
    let new_bytes = edit_one_line(&old_bytes);
    let old: File = parse_file(&old_bytes);
    let new: File = parse_file(&new_bytes);
    let path = FilePath::new("todo.txt").unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(old.lines.len(), LINES);
    let mut n = 0x0F00_0000u128;
    c.bench_function("reconcile_10k_one_edit", |b| {
        b.iter(|| {
            let mut mint = || {
                n += 1;
                task_id(n)
            };
            let r = reconcile(black_box(&old), black_box(&new), &path, &mut mint);
            assert_eq!(r.ops.len(), 1, "one edit_text op");
            r
        })
    });
    c.bench_function("attribution_diff_lines_10k", |b| {
        b.iter(|| txtodo_core::diff_lines(black_box(&old), black_box(&new)))
    });
    c.bench_function("attribution_id_of_10k", |b| {
        b.iter(|| old.lines.iter().filter_map(id_of).count())
    });
    c.bench_function("parse_and_reconcile_10k", |b| {
        b.iter(|| {
            let mut mint = || {
                n += 1;
                task_id(n)
            };
            let o = parse_file(black_box(&old_bytes));
            let nw = parse_file(black_box(&new_bytes));
            reconcile(&o, &nw, &path, &mut mint)
        })
    });
}

criterion_group!(benches, bench);
criterion_main!(benches);
