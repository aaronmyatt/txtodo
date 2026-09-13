// Dev-only in-browser stand-in for the Tauri command bridge (see `../tauriShim.ts` and
// `./tauriMock.ts`). This module owns the mutable state a mock daemon session needs — the event
// bus, the seeded workspace, tokens, and the activity log — so `./logic.ts` and `./tauriMock.ts`
// can both read/write it without a circular import. Never bundled into the real app path:
// `tauriShim` only reaches here when `window.__TAURI_INTERNALS__` is absent.
import type { Event, EventCallback, UnlistenFn } from "@tauri-apps/api/event";

export function delay(ms: number): Promise<void> {
	return new Promise((resolve) => setTimeout(resolve, ms));
}

// A fixed instant (not `Date.now()`) so mock timestamps/dates are stable across reloads.
export const NOW_MS = Date.UTC(2026, 8, 13, 15, 30, 0);
export const TODAY = "2026-09-13";

let ulidCounter = 0;
export function fakeUlid(): string {
	ulidCounter += 1;
	return `01MOCK${String(ulidCounter).padStart(20, "0")}`;
}

// ---- event bus (mockListen/emit) ----

type Listener<T> = (payload: T) => void;
const listeners = new Map<string, Set<Listener<unknown>>>();
let eventIdCounter = 0;

export function emit<T>(eventName: string, payload: T): void {
	for (const cb of listeners.get(eventName) ?? []) (cb as Listener<T>)(payload);
}

export function mockListen<T>(eventName: string, handler: EventCallback<T>): Promise<UnlistenFn> {
	eventIdCounter += 1;
	const id = eventIdCounter;
	const wrapped: Listener<unknown> = (payload) => handler({ event: eventName, id, payload } as Event<T>);
	let set = listeners.get(eventName);
	if (!set) {
		set = new Set();
		listeners.set(eventName, set);
	}
	set.add(wrapped);
	return Promise.resolve(() => set?.delete(wrapped));
}

// ---- in-memory workspace: two documents, one linked by `ref:release-notes` ----

export interface StoredFile {
	path: string;
	kind: string;
	text: string;
	hashSeq: number;
}

export function hashOf(f: Pick<StoredFile, "path" | "hashSeq">): string {
	return `mockhash-${f.path}-${f.hashSeq}`;
}

export const files = new Map<string, StoredFile>([
	[
		"todo.txt",
		{
			path: "todo.txt",
			kind: "todo",
			hashSeq: 0,
			text: [
				"(A) 2026-09-11 Ship the auth rewrite +authrewrite @work due:2026-09-19",
				"Reply to design feedback on the login screen +authrewrite @work id:01MOCKTASK00000000000001",
				"x 2026-09-10 2026-09-04 Book dentist @phone +health",
				"(B) 2026-09-12 Write the release notes +authrewrite @work ref:release-notes",
				"Buy oat milk @errands",
				""
			].join("\n")
		}
	],
	[
		"release-notes/todo.txt",
		{
			path: "release-notes/todo.txt",
			kind: "todo",
			hashSeq: 0,
			text: [
				"x 2026-09-12 2026-09-11 Draft the v2 highlights section +authrewrite",
				"x 2026-09-12 2026-09-11 List breaking changes +authrewrite",
				"(C) 2026-09-12 Screenshot the new settings page +authrewrite",
				"Get a second pass from design +authrewrite",
				""
			].join("\n")
		}
	],
	[
		"release-notes/notes.md",
		{ path: "release-notes/notes.md", kind: "notes", hashSeq: 0, text: "Ship target: 2026-09-19.\n" }
	]
]);

function progressOf(dirTodoPath: string): { done: number; total: number } {
	const f = files.get(dirTodoPath);
	if (!f) return { done: 0, total: 0 };
	const lines = f.text.split("\n").filter((l) => l.trim().length > 0);
	const done = lines.filter((l) => /^x \d{4}-\d{2}-\d{2}/.test(l)).length;
	return { done, total: lines.length };
}

export function listFilesDto() {
	return [...files.values()].map((f) => {
		const { done, total } = f.kind === "todo" ? progressOf(f.path) : { done: 0, total: 0 };
		return { path: f.path, hash: hashOf(f), kind: f.kind, done, total };
	});
}

// ---- notes (one per task_id, lazily created on first `edit_notes` — mirrors the daemon) ----

export const WORKSPACE_ROOT = "/Users/mock/workspace";

interface StoredNotes {
	path: string;
	text: string;
	hashSeq: number;
}

export const notesByTaskId = new Map<string, StoredNotes>();

// ---- conflicts (one seeded `needs_review` flag on the root file, for the banner/sheet design) ----

export let conflicts = [
	{
		task_id: "01MOCKTASK00000000000001",
		line_number: 2,
		mine: "Reply to design feedback on the login screen +authrewrite @work",
		theirs: "Reply to design feedback on the login copy +authrewrite @work"
	}
];

export function setConflicts(next: typeof conflicts): void {
	conflicts = next;
}

// ---- capability tokens ----

export interface StoredToken {
	id: string;
	name: string;
	scopes: string[];
	expires: string;
	created_at_ms: number;
	secret: string;
}

export const tokens: StoredToken[] = [
	{
		id: fakeUlid(),
		name: "release-notes bot",
		scopes: ["write:add", "write:complete", "project:authrewrite"],
		expires: "",
		created_at_ms: NOW_MS - 86_400_000,
		secret: ""
	}
];

// ---- activity feed ----
// `opLog` backs both `history()` (daemon.ts's `OpSummary` shape) and `op_log()`
// (devices/types.ts's differently-shaped `OpEvent`) — each entry carries every field either needs.

export interface OpLogEntry {
	seq: number;
	op_id: string;
	device: string;
	principal: string;
	kind: string;
	task_id: string;
	summary: string;
	at_ms: number;
}

export const opLog: OpLogEntry[] = [
	{
		seq: 3,
		op_id: fakeUlid(),
		device: "this-device",
		principal: "you@this-device",
		kind: "edit",
		task_id: "01MOCKTASK00000000000001",
		summary: 'edited "Reply to design feedback..."',
		at_ms: NOW_MS - 5 * 60_000
	},
	{
		seq: 2,
		op_id: fakeUlid(),
		device: "phone",
		principal: "you@phone",
		kind: "complete",
		task_id: "01MOCKTASK00000000000002",
		summary: 'completed "Book dentist"',
		at_ms: NOW_MS - 3 * 3_600_000
	},
	{
		seq: 1,
		op_id: fakeUlid(),
		device: "this-device",
		principal: "agent:release-bot@this-device",
		kind: "add",
		task_id: "01MOCKTASK00000000000003",
		summary: 'added "Write the release notes"',
		at_ms: NOW_MS - 26 * 3_600_000
	}
];

let hlcCounter = 0;
export function nextHlcCounter(): number {
	hlcCounter += 1;
	return hlcCounter;
}

export let lastSas = "";
export function setLastSas(sas: string): void {
	lastSas = sas;
}
