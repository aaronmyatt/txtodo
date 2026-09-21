// Thin typed wrappers around the Tauri command bridge (src-tauri/src/commands.rs). This is the
// ONLY place the frontend talks to the daemon: no `fs`, no `path`, no socket — every read/write
// crosses through these `invoke` calls (design §7).
// Ref: https://v2.tauri.app/develop/calling-rust/ and https://v2.tauri.app/develop/calling-frontend/
import { invoke, listen, type UnlistenFn } from "./tauriShim";
import type { BuildInfo } from "./versionInfo";
import { retryWhileLoading } from "./loadingRetry";

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
	return retryWhileLoading(() => invoke<FileInfo[]>("list_files"));
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
	/** One entry per line of `text`: the line's task id, `""` for a blank line. Absent or empty
	 * from a daemon older than the field (task sidecar-task-ids) — use `taskIdAt`, not this. */
	task_ids?: string[];
}

/** The exact bytes the daemon holds for one workspace-relative document path. */
export function getFile(path: string): Promise<FileContents> {
	return retryWhileLoading(() => invoke<FileContents>("get_file", { path }));
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
	| { kind: "delete"; task: TaskRef; leave_blank: boolean }
	// The whole document, compare-and-swap against `base_hash` (`FileContents.hash`). Must be the
	// only mutation of its `applyMutations` call. See `$lib/todotxt/saveBuffer.ts`.
	| { kind: "replace"; base_hash: string; contents: string };

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

/** This app's build and, when it answers, the daemon's (`commands_version.rs::build_info`). */
export function buildInfo(): Promise<BuildInfo> {
	return invoke("build_info");
}

/** Intent-level mutations on one workspace-relative document; the daemon turns them into ops.
 * `workspaceRoot` is the root the caller's buffer was read under: the bridge refuses the call
 * (`workspace-changed:`) when the app has switched workspace since, so a buffer saved during a
 * switch can never land in the other workspace's file. Omit it for a call that has no buffer.
 * Tauri maps this camelCase key to the command's `workspace_root` argument:
 * https://v2.tauri.app/develop/calling-rust/#passing-arguments */
export function applyMutations(path: string, mutations: Mutation[], workspaceRoot?: string): Promise<ApplyResult> {
	return invoke("apply", { path, mutations, workspaceRoot: workspaceRoot ?? null });
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

/** Ensures the one shared `Watch` stream (every document) is forwarding each change as a
 * `daemon-change` event (see `onDaemonChange`) — every listener filters to the path(s) it cares
 * about itself. Resolves once the stream is established, not when it ends — safe, and cheap, to
 * call more than once (e.g. once per open document): the backend only opens the stream on the
 * first call after a connection (tasks/desktop-concurrent-edit-loss root cause 3 — this used to
 * open one never-cancelled forwarder per call, so one change fired several `refreshDoc`s).
 * Ref: https://v2.tauri.app/develop/calling-rust/ */
export function watch(): Promise<void> {
	return invoke("watch");
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

/** `notes.md` for one task's `ref:` directory (plan M5). Mirrors `desktop_lib::dto::NotesDocDto`.
 * `text`/`path` are empty until the first `editNotes` call — the daemon lazily mints the `ref:`
 * tag and directory on that first write (`crates/txtodo-daemon/src/refdir_ops.rs::ensure_ref_dir`),
 * never the client (design §7). */
export interface NotesDoc {
	path: string;
	text: string;
	hash: string;
}

/** `notes.md` for one task's `ref:` directory; the daemon resolves the task to its directory by
 * `task.task_id` alone — no slug is ever computed or sent by this client. */
export function getNotes(task: TaskRef): Promise<NotesDoc> {
	return invoke("get_notes", { task });
}

/** Whole-document replacement of one task's `notes.md`; lazily creates the `ref:` tag and
 * directory on the first call for a task that doesn't have one yet (plan §3.2.4). */
export function editNotes(task: TaskRef, newText: string): Promise<ApplyResult> {
	return invoke("edit_notes", { task, newText });
}

/** Absolute workspace root, for the detail view's footer (display only — see
 * `src-tauri/src/commands.rs::workspace_root`'s doc comment for why this is still design-§7-safe).
 * Reflects whichever workspace `switchWorkspace` last selected, not a fixed startup value. */
export function workspaceRoot(): Promise<string> {
	return invoke("workspace_root");
}

/** Advisory-only: true when no `txtodo skill install` has run yet on this machine. Mirrors
 * `txtodo-cli`'s `doctor` skill row and `txtodo-tui`'s status-line hint. */
export function skillHintNeeded(): Promise<boolean> {
	return invoke("skill_hint");
}

/** One entry in the device-global workspace registry (ADR 0025). Mirrors
 * `desktop_lib::dto::WorkspaceInfoDto`. */
export interface WorkspaceInfo {
	id: string;
	root: string;
	added_at_ms: number;
	root_exists: boolean;
	has_state: boolean;
	/** How far the daemon is through opening it (task daemon-early-bind); "unknown" from an older daemon. */
	load_state: "queued" | "loading" | "ready" | "failed" | "unknown";
	/** Why the last open failed; empty unless `load_state` is "failed". */
	load_error: string;
	/** The user's default workspace (task default-workspace): labelled, and never removable. */
	is_default?: boolean;
}

/** Every registered workspace, oldest first. */
export function listWorkspaces(): Promise<WorkspaceInfo[]> {
	return invoke("list_workspaces");
}

/** Registers `root` (idempotent) without switching to it or opening it. */
export function addWorkspace(root: string): Promise<WorkspaceInfo> {
	return invoke("add_workspace", { root });
}

/** Un-registers a workspace id; never touches its `.txtodo/` state on disk. The daemon refuses
 * this for whichever workspace `root` names as the currently switched-to one — switch away
 * first. `false` means `id` was never a known workspace; `true` covers both a fresh removal and
 * removing an already-removed id (`Registry::remove` is an idempotent upsert-tombstone). */
export function removeWorkspace(id: string, root: string): Promise<boolean> {
	return invoke("remove_workspace", { id, root });
}

/** Points every future daemon call at `root` instead — no reconnect. Callers must also re-fetch
 * (and re-`watch`) anything already loaded against the old workspace: this only changes the
 * selector attached to calls made *after* it resolves, not any stream already open. */
export function switchWorkspace(root: string): Promise<void> {
	return invoke("switch_workspace", { root });
}

/** One `op_log_all()` row: an `OpEvent` (see `src/devices/types.ts`) tagged with its source
 * workspace — the field `OpLogEntry` itself doesn't carry (task
 * `desktop-activity-cross-workspace`; tagged client-side, not on the wire). Mirrors
 * `desktop_lib::dto::AggregatedOpLogEntryDto`. */
export interface AggregatedOpLogEntry {
	/** `"you@dev"` / `"agent:name@dev"` / `"external@dev"`. */
	principal: string;
	/** One-line human summary of the mutation. */
	op: string;
	/** Unix ms. */
	at_ms: number;
	/** `cli` / `tui` / `desktop` / `mcp` / `sync` / `external`; empty or absent for an old op. */
	source?: string;
	workspace_id: string;
	workspace_root: string;
}

/** Newest ops across every *registered* workspace (not just the one open in the main view),
 * merge-sorted newest-first and capped at 200 — same one-shot bounded-fetch contract as
 * `src/devices/api.ts::opLog()`'s single-workspace feed, not a live tail. A workspace whose root
 * no longer exists, or whose fetch fails, is silently skipped rather than failing the whole call. */
export function opLogAll(): Promise<AggregatedOpLogEntry[]> {
	return invoke("op_log_all");
}

/** One open (`!completed`) task line from some workspace's root `todo.txt`, tagged with enough
 * workspace identity to switch to it and label it. Mirrors
 * `desktop_lib::dto_universal::UniversalTaskDto`. */
export interface UniversalTask {
	workspace_id: string;
	workspace_root: string;
	line_number: number;
	priority: string | null;
	contexts: string[];
	description: string;
}

/** Every open task across every registered workspace's root `todo.txt` (ADR 0025, task
 * desktop-universal-view), unsorted — grouping by priority and filtering by `@context` are the
 * caller's job, over this flat list. Nested `ref:` sub-lists/notes aren't included (deliberate
 * scope cut, see todo.txt's `desktop-universal-view` entry); a workspace whose fetch fails is
 * silently absent from the result rather than failing the whole call. */
export function universalTasks(): Promise<UniversalTask[]> {
	return invoke("universal_tasks");
}

/** Tells the quick-add global hotkey's guard whether the main window's popover has an unsaved
 * edit right now (tasks/desktop-quick-add/notes.md: "if the main popover is open and dirty, the
 * hotkey focuses the main window instead"). Best-effort: callers swallow the rejection rather than
 * surfacing it, since a missed update here only affects which window the hotkey focuses next. */
export function setMainPopoverDirty(dirty: boolean): Promise<void> {
	return invoke("set_main_popover_dirty", { dirty });
}
