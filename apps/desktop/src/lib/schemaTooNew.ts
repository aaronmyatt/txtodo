// Turns the daemon's "database schema version N is newer than supported M" refusal into a plain
// message (task desktop-schema-too-new). Pure: no DOM, no Tauri.
// The trigger, 2026-09-21: a newer `txtodod` upgraded a list's store to schema 8, then the app's
// older bundled daemon (schema 7) was asked to open it and the raw RPC text reached the screen.
// Matching the text is the only seam that works: the old daemon is the one that fails, so it
// cannot be taught a new error code. Wording source: `StoreError::SchemaTooNew` in
// crates/txtodo-store/src/error.rs.
// RegExp: https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/RegExp

const SCHEMA_TOO_NEW = /database schema version (\d+) is newer than supported (\d+)/;

/** The plain message for a daemon error that is the schema refusal, or `null` for any other error. */
export function friendlySchemaError(error: unknown): string | null {
	const match = SCHEMA_TOO_NEW.exec(String(error));
	if (!match) return null;
	const [, found, supported] = match;
	return (
		`This list was upgraded by a newer txtodo (data format ${found}; this app reads up to ${supported}). ` +
		"Update the app, then open it again."
	);
}
