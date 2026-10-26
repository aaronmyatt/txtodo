// Stand-in for `@tauri-apps/plugin-dialog` in the web build (vite.config.ts's alias; ADR 0042): a
// browser workspace is not a folder, so there is nothing to pick. `null` is what the real `open`
// returns when the user cancels, which every caller already handles.
// Ref: https://v2.tauri.app/reference/javascript/dialog/#open

/** Mirrors the dialog plugin's `open`: always "cancelled" here. */
export async function open(_options?: Record<string, unknown>): Promise<null> {
	return null;
}
