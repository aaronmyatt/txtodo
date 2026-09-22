// Pure(ish) mock-daemon behaviour, split out of `./tauriMock.ts` so no single file carries both the
// seeded data (`./state.ts`) and the command logic. Mirrors the real daemon's `Apply`/pairing/token
// semantics closely enough for UI iteration — never a source of truth (see `./state.ts`'s header).
import {
	emit,
	fakeUlid,
	files,
	hashOf,
	lastSas,
	nextHlcCounter,
	notesByTaskId,
	NOW_MS,
	opLog,
	setLastSas,
	TODAY,
	tokens,
	type StoredToken
} from "./state";

export interface TaskRef {
	line_number: number;
	task_id: string;
}

export type Mutation =
	| { kind: "add"; line: string }
	| { kind: "complete"; task: TaskRef; today: string }
	| { kind: "edit"; task: TaskRef; new_line: string }
	| { kind: "move"; task: TaskRef; to_path: string }
	| { kind: "delete"; task: TaskRef; leave_blank: boolean }
	| { kind: "replace"; base_hash: string; contents: string };

function stampNewLine(line: string): string {
	const hasDate = /^(\(\w\) )?\d{4}-\d{2}-\d{2}\b/.test(line);
	const withDate = hasDate ? line : `${TODAY} ${line}`;
	return /\bid:\S+/.test(withDate) ? withDate : `${withDate} id:${fakeUlid()}`;
}

function applyOneMutation(lines: string[], m: Mutation): string[] {
	const idx = "task" in m ? m.task.line_number - 1 : -1;
	switch (m.kind) {
		case "add": {
			const stamped = stampNewLine(m.line);
			const trailingBlank = lines.length > 0 && lines[lines.length - 1] === "";
			if (trailingBlank) lines.splice(lines.length - 1, 0, stamped);
			else lines.push(stamped);
			return lines;
		}
		case "complete":
			if (idx >= 0 && idx < lines.length && !/^x \d{4}-\d{2}-\d{2}/.test(lines[idx])) {
				lines[idx] = `x ${m.today} ${lines[idx]}`;
			}
			return lines;
		case "edit":
			if (idx >= 0 && idx < lines.length) lines[idx] = m.new_line;
			return lines;
		case "move":
			if (idx >= 0 && idx < lines.length) lines.splice(idx, 1);
			return lines;
		case "delete":
			if (idx >= 0 && idx < lines.length) {
				if (m.leave_blank) lines[idx] = "";
				else lines.splice(idx, 1);
			}
			return lines;
		case "replace":
			// The whole document; `applyMutations` has already checked the base hash.
			return m.contents.split("\n");
	}
}

function summarize(m: Mutation): string {
	switch (m.kind) {
		case "add":
			return `added "${m.line}"`;
		case "complete":
			return `completed line ${m.task.line_number}`;
		case "edit":
			return `edited line ${m.task.line_number}`;
		case "move":
			return `moved line ${m.task.line_number} to ${m.to_path}`;
		case "delete":
			return `deleted line ${m.task.line_number}`;
		case "replace":
			return "replaced the document";
	}
}

export interface ApplyResult {
	applied: number;
	hash: string;
	hlc_wall_ms: number;
	hlc_counter: number;
}

export function applyMutations(path: string, mutations: Mutation[]): ApplyResult {
	const f = files.get(path);
	if (!f) throw new Error(`mock daemon: unknown path "${path}"`);
	// The real daemon refuses a `Replace` whose base is stale (`FAILED_PRECONDITION`); the bridge
	// turns that into this token (`$lib/todotxt/saveBuffer.ts::FAILED_PRECONDITION_TOKEN`).
	const stale = mutations.some((m) => m.kind === "replace" && m.base_hash !== hashOf(f));
	if (stale) throw "failed-precondition: the document changed since it was read";
	let lines = f.text.split("\n");
	for (const m of mutations) lines = applyOneMutation(lines, m);
	f.text = lines.join("\n");
	f.hashSeq += 1;
	const hlcCounter = nextHlcCounter();

	const opsForLog = mutations.map((m) => ({
		seq: hlcCounter,
		op_id: fakeUlid(),
		device: "this-device",
		principal: "you@this-device",
		kind: m.kind,
		task_id: "task" in m ? m.task.task_id : "",
		summary: summarize(m),
		at_ms: NOW_MS + hlcCounter
	}));
	opLog.unshift(...opsForLog);

	emit("daemon-change", { path, hash: hashOf(f), ops: opsForLog, review: [] });
	return { applied: mutations.length, hash: hashOf(f), hlc_wall_ms: NOW_MS + hlcCounter, hlc_counter: hlcCounter };
}

// ---- pairing (illustrative values only — never real key material) ----

const SAS_WORDS = ["harbor", "violet", "cinder", "maple", "otter", "quartz"];

export function mockPairOffer() {
	return {
		device: fakeUlid(),
		group_id: fakeUlid(),
		x25519_pub: "mock-x25519-pub-key-not-real",
		endpoint: "",
		nonce: "mock-nonce-0001"
	};
}

export function mockPairAccept() {
	setLastSas(SAS_WORDS.join(" "));
	return { sas: lastSas };
}

export function mockPairConfirm() {
	if (!lastSas) setLastSas(SAS_WORDS.join(" "));
	return { sas: lastSas };
}

// ---- notes (`get_notes`/`edit_notes`) ----

export function mockGetNotes(taskId: string) {
	const n = notesByTaskId.get(taskId);
	return n ? { path: `${taskId}/notes.md`, text: n.text, hash: `mockhash-notes-${taskId}-${n.hashSeq}` } : { path: "", text: "", hash: "" };
}

export function mockEditNotes(taskId: string, newText: string): ApplyResult {
	const existing = notesByTaskId.get(taskId);
	const hashSeq = (existing?.hashSeq ?? -1) + 1;
	notesByTaskId.set(taskId, { path: `${taskId}/notes.md`, text: newText, hashSeq });
	const hlcCounter = nextHlcCounter();
	return { applied: 1, hash: `mockhash-notes-${taskId}-${hashSeq}`, hlc_wall_ms: NOW_MS + hlcCounter, hlc_counter: hlcCounter };
}

// ---- ref: directories (`ref_dir`; task desktop-sublist-start) ----

/** A slug the way the daemon would pick one: the line's first plain words, else the id. */
function slugFor(line: string, taskId: string): string {
	const words = line
		.replace(/^(x \d{4}-\d{2}-\d{2} )?(\(\w\) )?(\d{4}-\d{2}-\d{2} )?/, "")
		.split(/\s+/)
		.filter((w) => w !== "" && !/[:@+]/.test(w))
		.slice(0, 3)
		.map((w) => w.toLowerCase().replace(/[^a-z0-9]+/g, ""))
		.filter((w) => w !== "");
	return words.length > 0 ? words.join("-") : taskId.toLowerCase();
}

/** Where the mock keeps a root-list line's ref dirs: beside the list, matching the seeded
 * `release-notes/` tree in state.ts. One constant, read by `workspace_layout` and `ref_dir`. */
export const MOCK_REFS_DIR = ".";

export function mockRefDir(path: string, task: TaskRef, ensure: boolean) {
	const f = files.get(path);
	if (!f) throw new Error(`mock daemon: unknown path "${path}"`);
	const line = f.text.split("\n")[task.line_number - 1] ?? "";
	const existing = /\bref:(\S+)/.exec(line)?.[1];
	const slug = existing ?? slugFor(line, task.task_id);
	// Composed the way `refDirFor` does from the mock's own `workspace_layout` answer.
	const dir = MOCK_REFS_DIR === "." ? slug : `${MOCK_REFS_DIR}/${slug}`;
	const subList = `${dir}/todo.txt`;
	if (ensure) {
		if (existing === undefined) applyMutations(path, [{ kind: "edit", task, new_line: `${line} ref:${slug}` }]);
		if (!files.has(subList)) files.set(subList, { path: subList, kind: "todo", hashSeq: 0, text: "" });
	}
	return { task_id: task.task_id, slug, dir, has_ref_tag: existing !== undefined, dir_exists: files.has(subList) };
}

// ---- capability tokens ----

export function mockTokenCreate(args: Record<string, unknown> | undefined): StoredToken {
	const token: StoredToken = {
		id: fakeUlid(),
		name: String(args?.name ?? ""),
		scopes: (args?.scopes as string[] | undefined) ?? [],
		expires: String(args?.expires ?? ""),
		created_at_ms: NOW_MS,
		secret: `mock-secret-${fakeUlid()}`
	};
	tokens.push(token);
	return token;
}

export function mockTokenRevoke(args: Record<string, unknown> | undefined): boolean {
	const id = args?.id as string;
	const idx = tokens.findIndex((t) => t.id === id);
	if (idx === -1) return false;
	tokens.splice(idx, 1);
	return true;
}
