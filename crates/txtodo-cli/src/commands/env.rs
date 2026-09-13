//! `txtodo env`: the resolved paths and config, one `key=value` per line or one JSON object. Split
//! out of `main.rs` to keep that file within its line budget.

use crate::config::validate_sync_dir;
use crate::{Ctx, json};

/// `sync_dir`'s validity suffix/flag: `None` when unconfigured, else whether
/// [`validate_sync_dir`] accepts it — a writable real directory, checked fresh every call since
/// this is external input that can change under the process (removed, unmounted, permissions).
fn sync_dir_problem(ctx: &Ctx) -> Option<Option<String>> {
    ctx.paths
        .sync_dir
        .as_deref()
        .map(|p| validate_sync_dir(p).err().map(|e| e.to_string()))
}

/// Prints every resolved path and config value.
pub fn run(ctx: &Ctx) {
    let schemes = ctx.config.url_schemes();
    let exists = if ctx.paths.config.exists() {
        ""
    } else {
        " (missing)"
    };
    let sync_dir = ctx
        .paths
        .sync_dir
        .as_ref()
        .map(|p| p.to_string_lossy().to_string());
    let sync_dir_problem = sync_dir_problem(ctx).flatten();
    if ctx.json {
        let object = format!(
            r#"{{"todo_dir":{},"todo_file":{},"report_file":{},"config_file":{},"config_exists":{},"id_tags":{},"key_store":{},"sync_dir":{},"sync_dir_problem":{},"url_schemes":[{}]}}"#,
            json::str(&ctx.paths.dir.to_string_lossy()),
            json::str(&ctx.paths.todo.to_string_lossy()),
            json::str(&ctx.paths.report.to_string_lossy()),
            json::str(&ctx.paths.config.to_string_lossy()),
            exists.is_empty(),
            ctx.config.id_tags(),
            json::str(ctx.config.key_store_mode().name()),
            sync_dir
                .as_deref()
                .map_or_else(|| "null".to_string(), json::str),
            sync_dir_problem
                .as_deref()
                .map_or_else(|| "null".to_string(), json::str),
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
    println!("report_file={}", ctx.paths.report.display());
    println!("config_file={}{exists}", ctx.paths.config.display());
    println!("id_tags={}", ctx.config.id_tags());
    println!("key_store={}", ctx.config.key_store_mode().name());
    match (&sync_dir, &sync_dir_problem) {
        (None, _) => println!("sync_dir=(not set)"),
        (Some(p), None) => println!("sync_dir={p}"),
        (Some(p), Some(problem)) => println!("sync_dir={p} (invalid: {problem})"),
    }
    println!("url_schemes={}", schemes.join(","));
}
