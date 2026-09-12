// Pure, framework-free logic for interpreting one todo.txt line of text: where a completed
// line's struck-through description starts, where `id:`/`ref:` tags sit, and how a `ref:` tag
// resolves against the daemon's per-file progress. Kept dependency-free (no CodeMirror, no
// Tauri) so it's unit-testable on its own — see `../__tests__/lineInfo.test.ts`.
//
// This is a *display* approximation, not a parser: `core-parse-file` (Rust) is the authoritative
// grammar. We only need enough to place decorations, never to build a task data model — the
// document text stays the single source of truth (design §7, "file-as-UI-model").
//
// Spec: plan §3.1 and §3.2 (txtodo-implementation-plan.md), tasks/desktop-main-view/notes.md.

/** The subset of `list_files`/`watch`'s `FileInfoDto` that `ref:` resolution needs. */
export interface FileProgress {
	path: string;
	done: number;
	total: number;
}

/** Where a completed line's description starts (design: the `x` marker and dates are never struck). */
export interface CompletedLineInfo {
	/** Offset into the line text where striking should start; may equal `text.length` (nothing to strike). */
	descriptionStart: number;
}

const ISO_DATE = "\\d{4}-\\d{2}-\\d{2}";
// `x` + completion date + optional creation date. Lenient on purpose: a hand-edited line that
// merely starts with "x " but has no valid date is not ours to reject here (that's the popover's
// strict/lenient validation, plan §3.2), it's just also not "completed" for styling purposes.
const COMPLETED_LINE = new RegExp(`^x ${ISO_DATE}(?: ${ISO_DATE})?(?: |$)`);

/** Null when `text` isn't a completed line (doesn't start with `x` + a completion date). */
export function completedLineInfo(text: string): CompletedLineInfo | null {
	const match = COMPLETED_LINE.exec(text);
	return match ? { descriptionStart: match[0].length } : null;
}

/** One `id:<value>` occurrence, as an offset range within the line text. */
export interface IdTagRange {
	from: number;
	to: number;
}

const ID_TAG = /\bid:\S+/g;

/** Every `id:` tag occurrence in `text`, for the zero-width hiding decoration. */
export function findIdTagRanges(text: string): IdTagRange[] {
	const ranges: IdTagRange[] = [];
	ID_TAG.lastIndex = 0;
	let match: RegExpExecArray | null;
	while ((match = ID_TAG.exec(text))) {
		ranges.push({ from: match.index, to: match.index + match[0].length });
	}
	return ranges;
}

/** A `ref:<slug>` tag found on a line. */
export interface RefTag {
	slug: string;
}

// Slug grammar per plan §3.2.1: `[a-z0-9][a-z0-9._-]*`, no `/`, not `.`/`..` (the parser's job to
// reject those two as `invalid_ref`; we just don't match a lone `.`/`..` as a slug character run).
const REF_TAG = /\bref:([a-z0-9][a-z0-9._-]*)/;

/** The first `ref:` tag on the line, if any (§3.2 convention: at most one per line). */
export function findRefTag(text: string): RefTag | null {
	const match = REF_TAG.exec(text);
	return match ? { slug: match[1] } : null;
}

/** What to show trailing a `ref:` line: open/total counts, or a notes-only icon. */
export type RefIndicator = { kind: "progress"; done: number; total: number } | { kind: "notes" };

/** Workspace-relative POSIX dirname; `""` for a top-level path (paths are always `/`-separated). */
export function dirOf(path: string): string {
	const i = path.lastIndexOf("/");
	return i === -1 ? "" : path.slice(0, i);
}

/** Joins a (possibly empty) workspace-relative directory with a child name. */
export function joinPath(dir: string, name: string): string {
	return dir === "" ? name : `${dir}/${name}`;
}

/**
 * Resolves a `ref:<slug>` tag found on a line of `containingPath` against the daemon's file
 * list. `done`/`total` come straight from `FileInfoDto.progress` (plan §3.2.5) — never
 * recomputed here. `null` for a dangling ref (§3.2.9: tag present, directory missing/unsynced) —
 * not an error, just nothing to show yet.
 */
export function resolveRefIndicator(
	containingPath: string,
	slug: string,
	filesByPath: ReadonlyMap<string, FileProgress>
): RefIndicator | null {
	const dir = joinPath(dirOf(containingPath), slug);
	const todo = filesByPath.get(joinPath(dir, "todo.txt"));
	if (todo) return { kind: "progress", done: todo.done, total: todo.total };
	const notes = filesByPath.get(joinPath(dir, "notes.md"));
	if (notes) return { kind: "notes" };
	return null;
}
