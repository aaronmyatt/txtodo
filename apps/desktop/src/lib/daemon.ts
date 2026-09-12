// Thin typed wrappers around the Tauri command bridge (src-tauri/src/commands.rs). This is the
// ONLY place the frontend talks to the daemon: no `fs`, no `path`, no socket — every read/write
// crosses through these `invoke` calls (design §7).
// Ref: https://v2.tauri.app/develop/calling-rust/ and https://v2.tauri.app/develop/calling-frontend/
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

/** Mirrors `desktop_lib::status::DaemonStatus` (serde `rename_all = "snake_case"`). */
export type DaemonStatus = "connected" | "connecting" | "spawning" | "dead";

/** Mirrors `desktop_lib::dto::FileInfoDto`. */
export interface FileInfo {
	path: string;
	hash: string;
	kind: string;
	done: number;
	total: number;
}

/** Every synced document with its current projection hash (`ListFiles`). */
export function listFiles(): Promise<FileInfo[]> {
	return invoke("list_files");
}

/** Current connectivity state to `txtodod`, queryable without waiting for an event. */
export function daemonStatus(): Promise<DaemonStatus> {
	return invoke("daemon_status");
}

/** Retries the connect/spawn sequence; wired to the reconnect banner's retry button. */
export function retryConnect(): Promise<DaemonStatus> {
	return invoke("retry_connect");
}

/** Subscribes to `daemon-status` events, pushed on every status change. */
export function onDaemonStatus(cb: (status: DaemonStatus) => void): Promise<UnlistenFn> {
	return listen<DaemonStatus>("daemon-status", (event) => cb(event.payload));
}
