//! `txtodo env`: the resolved paths and config, one `key=value` per line or one JSON object. Split
//! out of `main.rs` to keep that file within its line budget.

use crate::{Ctx, json};

/// Prints every resolved path and config value.
pub fn run(ctx: &Ctx) {
    let schemes = ctx.config.url_schemes();
    let exists = if ctx.paths.config.exists() {
        ""
    } else {
        " (missing)"
    };
    if ctx.json {
        let object = format!(
            r#"{{"todo_dir":{},"todo_file":{},"done_file":{},"report_file":{},"config_file":{},"config_exists":{},"id_tags":{},"key_store":{},"url_schemes":[{}]}}"#,
            json::str(&ctx.paths.dir.to_string_lossy()),
            json::str(&ctx.paths.todo.to_string_lossy()),
            json::str(&ctx.paths.done.to_string_lossy()),
            json::str(&ctx.paths.report.to_string_lossy()),
            json::str(&ctx.paths.config.to_string_lossy()),
            exists.is_empty(),
            ctx.config.id_tags(),
            json::str(ctx.config.key_store_mode().name()),
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
        return;
    }
    println!("todo_dir={}", ctx.paths.dir.display());
    println!("todo_file={}", ctx.paths.todo.display());
    println!("done_file={}", ctx.paths.done.display());
    println!("report_file={}", ctx.paths.report.display());
    println!("config_file={}{exists}", ctx.paths.config.display());
    println!("id_tags={}", ctx.config.id_tags());
    println!("key_store={}", ctx.config.key_store_mode().name());
    println!("url_schemes={}", schemes.join(","));
}
