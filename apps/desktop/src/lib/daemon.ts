// Thin typed wrappers around the Tauri command bridge (src-tauri/src/commands.rs). This is the
// ONLY place the frontend talks to the daemon: no `fs`, no `path`, no socket — every read/write
// crosses through these `invoke` calls (design §7).
// Ref: https://v2.tauri.app/develop/calling-rust/ and https://v2.tauri.app/develop/calling-frontend/
import { invoke, listen, type UnlistenFn } from "./tauriShim";

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

/** A line addressed by line number and/or ULID task id (`""` when the line has none yet). Mirrors `desktop_lib::dto::TaskRefDto`. */
export interface TaskRef {
	line_number: number;
	task_id: string;
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

/** The one `MutationDto` variant the edit popover needs, as its own type so callers don't have to
 * narrow `Mutation`'s union. Structurally identical to `Mutation`'s `"edit"` arm. */
export type EditMutation = Extract<Mutation, { kind: "edit" }>;

/** Mirrors `desktop_lib::dto::ResolutionDto` (serde `rename_all = "snake_case"` on a fieldless
 * enum serializes as a bare string). `"merged"` keeps whatever is already in the file and only
 * clears the flag — see `ConflictReviewSheet`'s `resolve()` doc comment for why that is not the
 * same thing as this app's client-side merge *preview*. */
export type ResolveChoice = "mine" | "theirs" | "merged";

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

/** Whole-line replacement on one workspace-relative document (`Apply`); the daemon derives
 * field-level ops and rejects a stale write against a moved/deleted line via `task`. Thin
 * convenience over `applyMutations` for the edit popover's single-mutation case. */
export function applyEdit(path: string, mutation: EditMutation): Promise<ApplyResult> {
	return applyMutations(path, [mutation]);
}

/** One recorded op, as surfaced on a `Change`. Mirrors `desktop_lib::dto::OpSummaryDto`. `op_id`
 * is a ULID, whose first 10 chars embed the op's millisecond timestamp
 * (https://github.com/ulid/spec) — there's no separate timestamp field on the DTO today. */
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

/** Starts (or restarts) a `Watch` stream scoped to `paths` (every document when empty); each
 * change is forwarded as a `daemon-change` event (see `onDaemonChange`). Resolves once the stream
 * is established, not when it ends — safe to call more than once (e.g. once per open document).
 * Ref: https://v2.tauri.app/develop/calling-rust/ */
export function watch(paths: string[] = []): Promise<void> {
	return invoke("watch", { paths });
}

/** Alias for {@link watch} taking a required `paths` array; kept for `FileView.svelte`'s call
 * sites so neither needs renaming. */
export function watchPaths(paths: string[]): Promise<void> {
	return watch(paths);
}

/** Subscribes to `daemon-change` events forwarded from an active `watch()` stream. */
export function onDaemonChange(cb: (change: Change) => void): Promise<UnlistenFn> {
	return listen<Change>("daemon-change", (event) => cb(event.payload));
}

/** Open `needs_review` flags for one workspace-relative document (plan M4). The daemon is
 * authoritative for this list — callers should reconcile it against any client-accumulated state
 * on mount/refresh rather than trusting the accumulation alone (design §4.7). */
export function listConflicts(path: string): Promise<ReviewFlag[]> {
	return invoke("list_conflicts", { path });
}

/** Resolves one `needs_review` flag; maps to the daemon's `ResolveConflict` RPC, the same
 * `Apply(Edit)` path the edit popover uses, so attribution/sync/history stay automatic and the
 * op itself clears the flag server-side. */
export function resolveConflict(
	path: string,
	task: TaskRef,
	resolution: ResolveChoice
): Promise<ApplyResult> {
	return invoke("resolve", { path, task, resolution });
}
