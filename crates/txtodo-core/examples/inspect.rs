//! Drive the core by hand until the CLI exists (M2):
//!   cargo run -q -p txtodo-core --example inspect -- '(A) 2026-09-11 Call the plumber +house @phone due:2026-09-15'
//! Prints both parse modes, the views, the token spans, and a complete()/uncomplete() round trip.
// An example is an output path, like the CLI (plan §0): printing is the point here.
#![allow(clippy::print_stdout, clippy::expect_used)]

use txtodo_core::{Date, Edit, LineEnding, LineKind, Mode, OwnedLine, apply, parse_line, tokenize};

fn main() {
    let raw = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "x 2026-09-11 (A) task +demo ref:../escape".to_string());
    println!("raw: {raw:?}");
    for mode in [Mode::Strict, Mode::Lenient] {
        match parse_line(&raw, mode) {
            Err(e) => println!("{mode:?}: error: {e}"),
            Ok(line) => match line.kind {
                LineKind::Blank => println!("{mode:?}: blank line"),
                LineKind::Task(t) => {
                    println!(
                        "{mode:?}: completed={} completion={:?} creation={:?} priority={:?} description={:?} quirks={}",
                        t.completed,
                        t.completion_date.map(|d| d.to_string()),
                        t.creation_date.map(|d| d.to_string()),
                        t.priority.map(|p| p.as_char()),
                        t.description,
                        line.quirks
                    );
                    println!(
                        "  projects={:?} contexts={:?} tags={:?} id={:?} ref={:?}",
                        t.projects().collect::<Vec<_>>(),
                        t.contexts().collect::<Vec<_>>(),
                        t.tags().collect::<Vec<_>>(),
                        t.id().map(|u| u.to_string()),
                        t.ref_slug()
                    );
                }
            },
        }
    }
    let spans: Vec<String> = tokenize(&raw)
        .iter()
        .map(|s| format!("{:?}={:?}", s.kind, &raw[s.start..s.end]))
        .collect();
    println!("tokens: {}", spans.join(" "));
    let today = Date::parse("2026-09-11").expect("valid");
    let line = OwnedLine::from_bytes(raw.clone().into_bytes(), LineEnding::Lf);
    let done = apply(&line, &Edit::new().complete(today));
    let back = apply(&done, &Edit::new().uncomplete());
    println!("complete():   {:?}", done.raw().unwrap_or("<opaque>"));
    println!("uncomplete(): {:?}", back.raw().unwrap_or("<opaque>"));
}
