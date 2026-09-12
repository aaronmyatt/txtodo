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

/** A line addressed by line number and/or id, mirroring `dto::TaskRefDto`. */
export interface TaskRef {
	line_number: number;
	task_id: string;
}

/** The one `MutationDto` variant the edit popover needs (`dto::MutationDto::Edit`); the other
 * variants (`Add`/`Complete`/`Move`/`Delete`) belong to features outside this task. */
export interface EditMutation {
	kind: "edit";
	task: TaskRef;
	new_line: string;
}

/** Mirrors `desktop_lib::dto::ApplyResultDto`. */
export interface ApplyResult {
	applied: number;
	hash: string;
	hlc_wall_ms: number;
	hlc_counter: number;
}

/** Whole-line replacement on one workspace-relative document (`Apply`); the daemon derives
 * field-level ops and rejects a stale write against a moved/deleted line via `task`. */
export function applyEdit(path: string, mutation: EditMutation): Promise<ApplyResult> {
	return invoke("apply", { path, mutations: [mutation] });
}

/** Mirrors `desktop_lib::dto::OpSummaryDto`. `op_id` is a ULID, whose first 10 chars embed the
 * op's millisecond timestamp (https://github.com/ulid/spec) — there's no separate timestamp
 * field on the DTO today. */
export interface OpSummary {
	seq: number;
	op_id: string;
	device: string;
	principal: string;
	kind: string;
	task_id: string;
	summary: string;
}

/** Mirrors `desktop_lib::dto::HistoryDto`. */
export interface HistoryResult {
	ops: OpSummary[];
}

/** Ops newest first for one path/task (`History`); the edit popover's footer uses `limit: 1`.
 * Tauri maps camelCase JS invoke args to the command's snake_case parameter names, so `taskId`
 * here reaches `commands::history`'s `task_id`. Ref: https://v2.tauri.app/develop/calling-rust/ */
export function history(path: string, taskId: string, limit: number): Promise<HistoryResult> {
	return invoke("history", { path, taskId, limit });
}
