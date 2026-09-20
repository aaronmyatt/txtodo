// Which build this app is, and whether the daemon it talks to is the same one (task
// version-info). Pure: no DOM, no Tauri. The trigger, 2026-09-20: the installed app spawned its
// own bundled `txtodod` from before early bind, and nothing showed that it was old.

/** Mirrors `desktop_lib::commands_version::BuildInfoDto`. The daemon half is `""` when the daemon
 * is not connected; `daemon_release_date` is also `""` from a daemon older than the field. */
export interface BuildInfo {
	version: string;
	release_date: string;
	daemon_version: string;
	daemon_release_date: string;
}

/** `v0.0.2 · 2026-09-20`, the one form every UI shows (`0.0.2 (2026-09-20)` is for `--version`). */
export function versionLabel(version: string, releaseDate: string): string {
	return `v${version} · ${releaseDate}`;
}

/**
 * What to tell the human when the daemon is another build, or `null` when it is this app's build
 * or there is no daemon to compare with (the daemon-status banner already covers "no daemon").
 * A daemon that sends no release date is older than the field, so it counts as different: that is
 * the very case that started this.
 */
export function daemonMismatch(info: BuildInfo): string | null {
	if (!info.daemon_version) return null;
	if (info.daemon_version === info.version && info.daemon_release_date === info.release_date) return null;
	const daemon = info.daemon_release_date
		? versionLabel(info.daemon_version, info.daemon_release_date)
		: `v${info.daemon_version} (an older build: it sent no release date)`;
	return (
		`The running daemon is ${daemon}; this app is ${versionLabel(info.version, info.release_date)}. ` +
		"Reinstall the app (it starts its own bundled daemon), or run `txtodo daemon install` and then `txtodo daemon start`."
	);
}
