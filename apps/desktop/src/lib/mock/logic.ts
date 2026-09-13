// Pure(ish) mock-daemon behaviour, split out of `./tauriMock.ts` so no single file carries both the
// seeded data (`./state.ts`) and the command logic. Mirrors the real daemon's `Apply`/pairing/token
// semantics closely enough for UI iteration — never a source of truth (see `./state.ts`'s header).
import { emit, fakeUlid, files, hashOf, lastSas, nextHlcCounter, NOW_MS, opLog, setLastSas, TODAY, tokens, type StoredToken } from "./state";

export interface TaskRef {
	line_number: number;
	task_id: string;
}

export type Mutation =
	| { kind: "add"; line: string }
	| { kind: "complete"; task: TaskRef; today: string }
	| { kind: "edit"; task: TaskRef; new_line: string }
	| { kind: "move"; task: TaskRef; to_path: string }
	| { kind: "delete"; task: TaskRef; leave_blank: boolean };

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
