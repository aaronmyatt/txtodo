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

/** Mirrors `desktop_lib::dto::FileContentsDto` (`GetFile`'s response). */
export interface FileContents {
	path: string;
	text: string;
	hash: string;
}

/** The exact bytes the daemon holds for one workspace-relative document path. */
export function getFile(path: string): Promise<FileContents> {
	return invoke("get_file", { path });
}

/**
 * Starts (or restarts) a `Watch` stream scoped to `paths`; matching events arrive as
 * `daemon-change` (subscribe with {@link onDaemonChange}). Resolves once the stream is
 * established, not when it ends — mirrors `commands::watch`.
 */
export function watchPaths(paths: string[]): Promise<void> {
	return invoke("watch", { paths });
}

/** A line addressed by line number and/or ULID task id (`""` when the line has none yet). Mirrors `desktop_lib::dto::TaskRefDto`. */
export interface TaskRef {
	line_number: number;
	task_id: string;
}

/** One recorded op, as surfaced on a `Change`. Mirrors `desktop_lib::dto::OpSummaryDto`. */
export interface OpSummary {
	seq: number;
	op_id: string;
	device: string;
	principal: string;
	kind: string;
	task_id: string;
	summary: string;
}

/** One `needs_review` flag raised by a change. Mirrors `desktop_lib::dto::ReviewFlagDto`. */
export interface ReviewFlag {
	task_id: string;
	line_number: number;
	mine: string;
	theirs: string;
}

/** One `Watch` event. Mirrors `desktop_lib::dto::ChangeDto`. */
export interface Change {
	path: string;
	hash: string;
	ops: OpSummary[];
	review: ReviewFlag[];
}

/** Subscribes to `daemon-change` events forwarded from an active `watch()` stream. */
export function onDaemonChange(cb: (change: Change) => void): Promise<UnlistenFn> {
	return listen<Change>("daemon-change", (event) => cb(event.payload));
}

/**
 * One intent-level mutation `apply()` can make. Mirrors `desktop_lib::dto::MutationDto`
 * (`#[serde(tag = "kind", rename_all = "snake_case")]` — field names below are otherwise
 * untouched by serde, so they stay exactly as declared on the Rust side).
 */
export type Mutation =
	| { kind: "add"; line: string }
	| { kind: "complete"; task: TaskRef; today: string }
	| { kind: "edit"; task: TaskRef; new_line: string }
	| { kind: "move"; task: TaskRef; to_path: string }
	| { kind: "delete"; task: TaskRef; leave_blank: boolean };

/** Result of `apply()`/`resolve()`. Mirrors `desktop_lib::dto::ApplyResultDto`. */
export interface ApplyResult {
	applied: number;
	hash: string;
	hlc_wall_ms: number;
	hlc_counter: number;
}

/** Intent-level mutations on one workspace-relative document; the daemon turns them into ops. */
export function applyMutations(path: string, mutations: Mutation[]): Promise<ApplyResult> {
	return invoke("apply", { path, mutations });
}
