//! Split out of `commands.rs` purely for the file budget (task `desktop-stack-gaps`, discovered
//! once `check-file-length.sh` was widened to actually scan `apps/desktop/src-tauri` — see that
//! script's own comment). Bridges a frontend log call into this process's own tracing subscriber
//! (root todo.txt `logging-desktop`) so a UI-side event lands in the same `desktop.log.*`/stderr
//! this crate already writes to — the receiving end only; the frontend's own call sites (what's
//! safe to pass as `message`/`fields`) are a separate, later backlog item (`logging-frontend`).
//! `level` is one of `error`/`warn`/`info`/`debug`/`trace` (anything else logs at `info`);
//! `message` and `fields` are exactly what this command exists to carry into the log, so —
//! unlike every other command in the sibling file — they are deliberately *not* skipped: passing
//! them through *is* the feature, not an accidental content leak (CLAUDE.md's "never log task
//! line text" rule is about this crate's own spans/events incidentally capturing IPC argument
//! content, not about a purpose-built log bridge).

#[tracing::instrument(name = "ipc.ui_log", skip_all, fields(level = %level))]
#[tauri::command]
pub fn ui_log(
    level: String,
    message: String,
    fields: Option<serde_json::Value>,
) -> Result<(), String> {
    ui_log_inner(&level, &message, fields)
}

fn ui_log_inner(
    level: &str,
    message: &str,
    fields: Option<serde_json::Value>,
) -> Result<(), String> {
    // Serialized once, up front: `serde_json::Value::to_string()` is compact JSON (`Display`,
    // not `Debug`), so the field lands in the log as real JSON text instead of Rust debug syntax.
    let fields_json = fields
        .as_ref()
        .map_or_else(|| "null".to_owned(), ToString::to_string);
    match level {
        "error" => log_ui_error(message, &fields_json),
        "warn" => log_ui_warn(message, &fields_json),
        "debug" => log_ui_debug(message, &fields_json),
        "trace" => log_ui_trace(message, &fields_json),
        _ => log_ui_info(message, &fields_json),
    }
    Ok(())
}

/// One event-emitting function per level (never a bare `tracing::*!` call inside `ui_log_inner`'s
/// own `match` — that costs `clippy::cognitive_complexity` points, the same reason
/// `crates/txtodo-daemon/src/mutation.rs`'s `log_mutation_ops` is its own function).
fn log_ui_error(message: &str, fields_json: &str) {
    tracing::error!(fields = fields_json, "{message}");
}

fn log_ui_warn(message: &str, fields_json: &str) {
    tracing::warn!(fields = fields_json, "{message}");
}

fn log_ui_info(message: &str, fields_json: &str) {
    tracing::info!(fields = fields_json, "{message}");
}

fn log_ui_debug(message: &str, fields_json: &str) {
    tracing::debug!(fields = fields_json, "{message}");
}

fn log_ui_trace(message: &str, fields_json: &str) {
    tracing::trace!(fields = fields_json, "{message}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing_subscriber::fmt::format::FmtSpan;
    use tracing_subscriber::layer::SubscriberExt;

    /// Proves `ui_log`'s `ipc.ui_log` span and its forwarded event actually reach a real
    /// subscriber — not just "it compiled" — the same `txtodo_telemetry::testing::LogSink` +
    /// `with_span_events(FmtSpan::CLOSE)` pattern `crates/txtodo-mcp/tests/smoke.rs`'s
    /// `mcp_call_span_names_tool_and_records_principal` test uses (a span with no event inside it
    /// never otherwise reaches the writer). Also doubles as the empirical proof that
    /// `#[tracing::instrument]` above `#[tauri::command]` works for a Tauri command: `ui_log` is
    /// called directly here exactly as `generate_handler!` would call it.
    #[test]
    fn ui_log_emits_a_named_span_and_the_forwarded_message() {
        let sink = txtodo_telemetry::testing::LogSink::new();
        let subscriber = tracing_subscriber::registry().with(
            tracing_subscriber::fmt::layer()
                .json()
                .with_span_events(FmtSpan::CLOSE)
                .with_writer(sink.clone()),
        );
        let _guard = tracing::subscriber::set_default(subscriber);

        ui_log(
            "warn".to_owned(),
            "quick_add_hotkey_failed".to_owned(),
            Some(serde_json::json!({"attempt": 2})),
        )
        .expect("ui_log never fails");

        let text = sink.captured_text();
        let span_line = text
            .lines()
            .find(|l| l.contains("\"ipc.ui_log\""))
            .unwrap_or_else(|| panic!("no ipc.ui_log span line in captured output: {text}"));
        let span_value: serde_json::Value = serde_json::from_str(span_line).expect("valid JSON");
        assert_eq!(span_value["span"]["name"], "ipc.ui_log", "{span_line}");
        assert_eq!(span_value["span"]["level"], "warn", "{span_line}");
        let event_line = text
            .lines()
            .find(|l| l.contains("quick_add_hotkey_failed"))
            .unwrap_or_else(|| panic!("no forwarded message line in captured output: {text}"));
        let event_value: serde_json::Value = serde_json::from_str(event_line).expect("valid JSON");
        assert_eq!(
            event_value["fields"]["message"], "quick_add_hotkey_failed",
            "{event_line}"
        );
        // `fields` rides as a nested, already-serialized JSON string (see `ui_log_inner`), so it
        // is parsed a second time here rather than compared as a substring.
        let nested_fields: serde_json::Value = serde_json::from_str(
            event_value["fields"]["fields"]
                .as_str()
                .expect("string field"),
        )
        .expect("nested fields value is valid JSON");
        assert_eq!(nested_fields["attempt"], 2, "{event_line}");
    }
}
