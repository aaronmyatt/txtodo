//! `txtodo env`: prints the resolved paths and config. Split out of `main.rs` (rather than left
//! inline) purely to keep that file's clap wiring under the file-length budget — every other
//! dispatched command already has its own `commands::*` module.

use crate::{Ctx, json};

/// One `key=value` per line, or one JSON object with `--json`.
pub fn run(ctx: &Ctx) {
    let schemes = ctx.config.url_schemes();
    let exists = if ctx.paths.config.exists() {
        ""
    } else {
        " (missing)"
    };
    if ctx.json {
        print_json(ctx, &schemes, exists);
        return;
    }
    println!("todo_dir={}", ctx.paths.dir.display());
    println!("todo_file={}", ctx.paths.todo.display());
    println!("done_file={}", ctx.paths.done.display());
    println!("report_file={}", ctx.paths.report.display());
    println!("config_file={}{exists}", ctx.paths.config.display());
    println!("id_tags={}", ctx.config.id_tags());
    println!("url_schemes={}", schemes.join(","));
}

fn print_json(ctx: &Ctx, schemes: &[String], exists: &str) {
    let object = format!(
        r#"{{"todo_dir":{},"todo_file":{},"done_file":{},"report_file":{},"config_file":{},"config_exists":{},"id_tags":{},"url_schemes":[{}]}}"#,
        json::str(&ctx.paths.dir.to_string_lossy()),
        json::str(&ctx.paths.todo.to_string_lossy()),
        json::str(&ctx.paths.done.to_string_lossy()),
        json::str(&ctx.paths.report.to_string_lossy()),
        json::str(&ctx.paths.config.to_string_lossy()),
        exists.is_empty(),
        ctx.config.id_tags(),
        schemes
            .iter()
            .map(|s| json::str(s))
            .collect::<Vec<_>>()
            .join(",")
    );
    debug_assert!(
        object.starts_with('{') && object.ends_with('}'),
        "one object"
    );
    println!("{object}");
}
