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

/** Mirrors `desktop_lib::dto::ReviewFlagDto` (plan M4): one `needs_review` flag — two devices
 * rewrote the same word offline (design §4.7: "char-level merge, flagged for a one-tap review"). */
export interface ReviewFlag {
	task_id: string;
	line_number: number;
	mine: string;
	theirs: string;
}

/** Mirrors `desktop_lib::dto::TaskRefDto`: a line addressed by 1-based line number and/or ULID. */
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

/** Mirrors `desktop_lib::dto::ResolutionDto` (serde `rename_all = "snake_case"` on a fieldless
 * enum serializes as a bare string). `"merged"` keeps whatever is already in the file and only
 * clears the flag — see `ConflictReviewSheet`'s `resolve()` doc comment for why that is not the
 * same thing as this app's client-side merge *preview*. */
export type ResolveChoice = "mine" | "theirs" | "merged";

/** Mirrors `desktop_lib::dto::ApplyResultDto` (`Apply`/`ResolveConflict` result). */
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

/** Mirrors `desktop_lib::dto::ChangeDto`, forwarded from a `Watch` stream as a `daemon-change`
 * event (`commands.rs::watch`). */
export interface Change {
	path: string;
	hash: string;
	ops: OpSummary[];
	review: ReviewFlag[];
}

/** Starts a `Watch` stream for `paths` (every document when empty); each change is forwarded as
 * a `daemon-change` event (see `onDaemonChange`). Resolves once the stream is established, not
 * when it ends — safe to call more than once (e.g. once per open document).
 * Ref: https://v2.tauri.app/develop/calling-rust/ */
export function watch(paths: string[] = []): Promise<void> {
	return invoke("watch", { paths });
}

/** Subscribes to `daemon-change` events pushed by an active `watch()` stream. */
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
