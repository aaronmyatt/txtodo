<script lang="ts">
	// THE reusable file-rendering component (tasks/desktop-main-view). Binds to the one shared
	// `watch()` stream, rebuilds from `get_file(path)` on each matching `Change`, and renders a
	// CM6 `EditorView`
	// (blanks included — design §2.6: blanks are entries) that is directly editable, like a plain
	// text file: click to drop a cursor, type, blur/Cmd-S commits. No popover, no separate "raw
	// mode" — the whole document is always the live buffer; `path`'s own doc comment on `dirty`
	// below covers how an edit gets back to the daemon as an intent-level `Mutation`, never a
	// whole-string replace.
	// `depth` is a hint only (nested indentation); nothing here assumes `depth === 0` — a future
	// detail-view task mounts this again at `depth + 1` for a `ref:` directory's todo.txt.
	//
	// CM6 basics: https://codemirror.net/docs/ref/
	import { dragLines, startLineDrag } from "$lib/todotxt/dragLines";
	import { newestChange, popChange, popIfNewest } from "$lib/stores/changes";
	import { showToast } from "$lib/stores/toast";
	import { listBuffer } from "$lib/listCommands";
	import { docLines, listMutations, moveTarget } from "$lib/todotxt/listMode";
	import { applyChipCore } from "$lib/wasmCore";
	import { localToday } from "./editPopoverLogic";
	import { leaveEditor } from "$lib/escape";
	import { onDestroy, onMount } from "svelte";
	import { Compartment, EditorState } from "@codemirror/state";
	import {
		drawSelection,
		EditorView,
		highlightActiveLine,
		highlightActiveLineGutter,
		keymap,
		lineNumbers
	} from "@codemirror/view";
	import { defaultKeymap } from "@codemirror/commands";
	import { todotxtLanguage } from "$lib/lang/todotxtLanguage";
	import { get } from "svelte/store";
	import {
		applyMutations,
		getFile,
		listFiles,
		onDaemonChange,
		undoChange,
		watch,
		type FileInfo,
		type Mutation
	} from "$lib/daemon";
	import { dirOf } from "$lib/todotxt/lineInfo";
	import {
		idTagsHidden,
		lineDecorations,
		longLineHint,
		mainViewBaseTheme
	} from "$lib/todotxt/decorations";
	import { hoverLine, setHoverLine } from "$lib/todotxt/hoverLine";
	import { isReorderOnly, saveBuffer, type Baseline, type SaveOutcome } from "$lib/todotxt/saveBuffer";
	import { rejectedEdits } from "$lib/stores/rejectedEdits";
	import { currentWorkspaceRoot } from "$lib/stores/workspaces";
	import type { DetailParams } from "$lib/types";
	import { EMPTY_STATUS, editorStatus, SAVED_MS, type SaveState } from "$lib/stores/editorStatus";
	import { docCounts } from "$lib/todotxt/counts";
	import { editorPrefs } from "$lib/stores/editorPrefs";
	import { searchQuery, searchStep } from "$lib/stores/search";
	import { hitLines, search, setQuery, stepHit } from "$lib/todotxt/search";
	import { wasmReady } from "$lib/wasmCore";

	interface Props {
		path: string;
		depth: number;
		/** Stretches the editor to fill its container's height instead of the default capped
		 * card (see `.editor-shell` below) — the root view (`MainView`) wants the editor to occupy
		 * all remaining space; a nested detail-view sub-list sits among other sections on a normal
		 * scrolling page and keeps the capped look. */
		fill?: boolean;
		/** Optional: fires whenever this instance's buffer becomes dirty/clean, so a host (MainView)
		 * can tell the quick-add hotkey's guard (tasks/desktop-quick-add/notes.md: "if the main
		 * window has an unsaved edit, the hotkey focuses it instead"). */
		onDirtyChange?: (dirty: boolean) => void;
		/** Double-click (or Cmd/Ctrl+Enter) on a line (tasks/desktop-detail-view, plan §3.2): opens
		 * the detail view for that line, whether or not it has a `ref:` tag yet — a task with none
		 * still lazily creates one on the first notes/sub-list edit (plan §3.2.4). Optional so a
		 * `FileView` used somewhere that never wants a detail view (there is none today, but the
		 * component itself shouldn't assume one always exists) doesn't have to pass a no-op. */
		onDetailRequest?: (params: DetailParams) => void;
		/** This is the main buffer: report its save state and caret to the status footer
		 * (`$lib/stores/editorStatus`). Only MainView's root view sets it. */
		reportStatus?: boolean;
	}

	let {
		path,
		depth,
		fill = false,
		onDirtyChange,
		onDetailRequest,
		reportStatus = false
	}: Props = $props();

	let savedTimer: ReturnType<typeof setTimeout> | undefined;
	/** Tells the footer the save state, when this is the main buffer. "saved" settles to
	 * "synced" after a moment. */
	function reportSave(save: SaveState) {
		if (!reportStatus) return;
		clearTimeout(savedTimer);
		clearTimeout(countTimer);
		editorStatus.update((s) => ({ ...s, save }));
		if (save === "saved") {
			savedTimer = setTimeout(() => editorStatus.update((s) => ({ ...s, save: dirty ? "unsaved" : "synced" })), SAVED_MS);
		}
	}

	let containerEl: HTMLDivElement | undefined = $state();
	let rootEl: HTMLDivElement | undefined = $state();
	// Whether list mode's keys act on this list (`$lib/listCommands.ts`); only that list shows its
	// current line, so with a detail open the eye can tell which one the keys move.
	let activeList = $state(false);
	let view: EditorView | undefined;
	let filesByPath = $state<Map<string, FileInfo>>(new Map());
	let loadError = $state("");
	// The daemon's refusal of this instance's last commit. Kept apart from `loadError` because
	// `refreshDoc` clears that one on every successful fetch, which used to erase a refusal the
	// moment the path-switch effect repainted; this one only clears on a later successful commit or
	// a discard (root todo id:01M2WK5DQQ9A3V3M4F2CJN0KMX).
	let commitError = $state("");
	// Testability hook only (tasks/desktop-visual-regression): the perf test waits on
	// `[data-line-count='10000']` to know the 10k-line fixture has actually reached the editor,
	// rather than guessing a fixed sleep. Kept to a single `$state` + one line in the template —
	// no behavior change for the shipped app.
	let docLineCount = $state(0);

	// The document is always a live, editable buffer (no separate "raw mode" to enter). `dirty`
	// tracks whether it currently differs from `baseline` — the last daemon-confirmed content —
	// and `baseline` is kept up to date by every successful `refreshDoc` while not dirty (never
	// carried across a `path` swap or an unmount unsaved — see the `$effect`/`onDestroy` below,
	// same invariant tasks/desktop-raw-mode's notes.md documented for the mode this replaced).
	let dirty = $state(false);
	let baseline = "";
	// The hash `GetFile` gave with `baseline`: the base a `Replace` save names, so the daemon can
	// refuse it when the document moved on (`$lib/todotxt/saveBuffer.ts`). Always set together
	// with `baseline`, never alone.
	let baselineHash = "";
	// The workspace root `baseline` was read under. A workspace switch remounts this component,
	// and its unmount save runs after the bridge has moved on to the new workspace: the save names
	// this root so the bridge can refuse it, and a refusal is parked under it (finding 4).
	let baselineWorkspace = "";
	// The daemon's task id per line of `baseline` (`FileContents.task_ids`), for list mode's moves.
	let baselineTaskIds: string[] = [];
	// A pending "the human only moved a line" save (task desktop-reorder-propagates): a reorder
	// reaches the file at once, typed text still waits for blur or Cmd-S.
	const REORDER_SAVE_DELAY_MS = 300;
	let reorderTimer: ReturnType<typeof setTimeout> | undefined;

	// Per-instance reconfigurable slots (never shared across FileView instances — see
	// $lib/todotxt/decorations.ts) so refreshing ref: progress is a cheap dispatch, not a full
	// document rebuild. The hovered line is a state field instead (`$lib/todotxt/hoverLine.ts`).
	const lineDecoCompartment = new Compartment();
	// The gutter and the long-line hint follow the editor prefs (`$lib/stores/editorPrefs`).
	const gutterCompartment = new Compartment();
	const lengthHintCompartment = new Compartment();
	// The line numbers double as drag handles (`$lib/todotxt/dragLines.ts`): press one, drag, drop.
	const gutterFor = (on: boolean) =>
		on
			? [
					lineNumbers({
						domEventHandlers: {
							mousedown: (v, block, event) => startLineDrag(v, v.state.doc.lineAt(block.from).number, event as MouseEvent)
						}
					}),
					highlightActiveLineGutter()
				]
			: [];
	const lengthHintFor = (on: boolean) => (on ? longLineHint : []);

	function setDirty(next: boolean) {
		if (next === dirty) return;
		dirty = next;
		onDirtyChange?.(next);
		if (next) reportSave("unsaved");
	}

	function initialExtensions() {
		return [
			// `Mod-Enter` (Cmd+Enter on macOS, Ctrl+Enter elsewhere — CM6's own convention:
			// https://codemirror.net/docs/ref/#commands) is the keyboard equivalent of a
			// double-click, opening the detail view for the line under the caret (plan §3.2).
			// `Mod-s` commits a dirty buffer early (otherwise blur does it); `Escape` discards it,
			// reverting to `baseline`, then leaves the editor (`$lib/escape.ts`: the shortcuts work
			// again and an open detail panel closes) — Escape is unbound elsewhere in the buffer.
			keymap.of([
				{ key: "Mod-Enter", run: openDetailAtSelection },
				{
					key: "Mod-s",
					run: () => {
						if (!dirty) return false;
						commit();
						return true;
					}
				},
				{
					key: "Escape",
					run: (v) => {
						if (dirty) discard();
						leaveEditor(v.contentDOM);
						return true;
					}
				},
				...defaultKeymap
			]),
			todotxtLanguage,
			mainViewBaseTheme,
			dragLines(),
			// Chrome (task desktop-ui-revamp): https://codemirror.net/docs/ref/#view.lineNumbers,
			// #view.highlightActiveLine, #view.drawSelection (draws the caret the theme styles).
			gutterCompartment.of(gutterFor(get(editorPrefs).lineNumbers)),
			search(),
			highlightActiveLine(),
			drawSelection(),
			idTagsHidden, // always hidden — §3.1
			lengthHintCompartment.of(lengthHintFor(get(editorPrefs).lengthHint)), // root todo 9
			lineDecoCompartment.of(lineDecorations(path, filesByPath)),
			hoverLine,
			EditorView.domEventHandlers({
				mousemove: handleMouseMove,
				mouseleave: (_event, editorView) => {
					clearHover(editorView);
				},
				dblclick: handleDblClick,
				// Blur commits (same rule tasks/desktop-raw-mode's notes.md stated: "on blur or
				// Cmd/Ctrl+S the buffer goes through the reconciler"). CM6's `blur` domEventHandler
				// fires on the real DOM blur of the content element, i.e. focus actually left the
				// editor — not on the transient focus shuffles a single click within it can cause.
				blur: () => {
					if (dirty) commit();
					return false;
				}
			}),
			EditorView.updateListener.of((u) => {
				if (reportStatus && (u.selectionSet || u.docChanged)) {
					const head = u.state.selection.main.head;
					const line = u.state.doc.lineAt(head);
					editorStatus.update((s) => ({ ...s, line: line.number, col: head - line.from + 1 }));
				}
				if (!u.docChanged) return;
				docLineCount = u.state.doc.lines;
				if (reportStatus) {
					const counts = docCounts(u.state.doc.toString());
					editorStatus.update((s) => ({ ...s, counts }));
					countHitsSoon();
				}
				const text = u.state.doc.toString();
				setDirty(text !== baseline);
				scheduleReorderSave(text);
			})
		];
	}

	function requestDetail(lineNumber: number) {
		onDetailRequest?.({ file: path, line: lineNumber });
	}

	function openDetailAtSelection(editorView: EditorView): boolean {
		if (!onDetailRequest) return false;
		const line = editorView.state.doc.lineAt(editorView.state.selection.main.head);
		requestDetail(line.number);
		return true;
	}

	function handleDblClick(event: MouseEvent, editorView: EditorView): boolean {
		const pos = editorView.posAtCoords({ x: event.clientX, y: event.clientY });
		if (pos == null) return false;
		requestDetail(editorView.state.doc.lineAt(pos).number);
		return true;
	}

	// The hovered line lives in editor state (`hoverLine`), which rebuilds its decoration from the
	// current document: a synced edit that shrinks the file under a still mouse cannot leave it
	// out of range (root todo "the hover highlight is never remapped").
	function clearHover(editorView: EditorView) {
		if (editorView.state.field(hoverLine) === null) return;
		editorView.dispatch({ effects: setHoverLine.of(null) });
	}

	function handleMouseMove(event: MouseEvent, editorView: EditorView): boolean {
		const pos = editorView.posAtCoords({ x: event.clientX, y: event.clientY });
		const lineNumber = pos == null ? null : editorView.state.doc.lineAt(pos).number;
		if (lineNumber === editorView.state.field(hoverLine)) return false;
		editorView.dispatch({ effects: setHoverLine.of(lineNumber) });
		return false;
	}

	/** A change that only moved lines saves after a short pause, without waiting for blur; any
	 * other edit cancels the pending save (typed text must never reach the disk half-written).
	 * setTimeout: https://developer.mozilla.org/en-US/docs/Web/API/Window/setTimeout */
	function scheduleReorderSave(text: string) {
		clearTimeout(reorderTimer);
		reorderTimer = undefined;
		if (!isReorderOnly(baseline, text)) return;
		reorderTimer = setTimeout(() => {
			reorderTimer = undefined;
			void commit();
		}, REORDER_SAVE_DELAY_MS);
	}

	/** The one way a buffer goes back to the daemon — blur, Cmd-S, a file switch and an unmount all
	 * come through here, against the path the buffer belongs to (not necessarily `path`: by the
	 * time the switch effect runs, `path` already holds the destination). Returns the daemon's
	 * refusal, or `null` on success. A refusal is parked in `rejectedEdits` with the exact text,
	 * so a caller that has no editor left to keep it in (switch, unmount) still leaves the human a
	 * way to get it back; `keptInEditor` skips that for a caller whose buffer stays on screen. A
	 * success clears any older refusal of the same file. */
	async function commitOutgoing(
		targetPath: string,
		base: Baseline,
		next: string,
		keptInEditor = false
	): Promise<string | null> {
		const workspace = baselineWorkspace;
		const apply = (p: string, m: Mutation[]) => applyMutations(p, m, workspace);
		try {
			// `Replace` first (a moved line is a move), the per-line delta when the base is stale:
			// `$lib/todotxt/saveBuffer.ts`. The daemon's `Change` repaints the view via `refreshDoc`.
			const outcome = await saveBuffer(apply, targetPath, base, next);
			adoptSaved(targetPath, base, next, outcome);
			rejectedEdits.clear(workspace, targetPath);
			return null;
		} catch (e) {
			const error = String(e);
			if (!keptInEditor) rejectedEdits.record({ workspace, path: targetPath, text: next, error });
			return error;
		}
	}

	/** After a `Replace` the daemon holds exactly `next`, so it is the new baseline even when the
	 * human typed on during the round trip and `refreshDoc` (which skips a dirty buffer) did not
	 * run. Without this the next save would name the old hash and be refused. Only while this
	 * instance still shows `targetPath` and no refresh moved the baseline meanwhile. The per-line
	 * fallback merges with someone else's change, so its result is not `next`: `refreshDoc` owns
	 * that case. */
	function adoptSaved(targetPath: string, base: Baseline, next: string, outcome: SaveOutcome) {
		if (outcome.how !== "replace" || targetPath !== path || baselineHash !== base.hash) return;
		baseline = next;
		baselineHash = outcome.hash;
		if (view) setDirty(view.state.doc.toString() !== baseline);
	}

	/** Blur/Cmd-S: commits the buffer against *this* instance's current `path`. */
	async function commit(): Promise<void> {
		clearTimeout(reorderTimer);
		reorderTimer = undefined;
		if (!view || !dirty) return;
		const next = view.state.doc.toString();
		setDirty(false);
		reportSave("saving");
		const error = await commitOutgoing(path, { text: baseline, hash: baselineHash }, next, true);
		if (error === null) {
			commitError = "";
			reportSave("saved");
			return;
		}
		reportSave("refused");
		// Keep the buffer dirty and untouched: the edit was rejected, not applied. Calling
		// refreshDoc() here would silently overwrite the human's text with the daemon's
		// pre-edit content (root cause 4, tasks/desktop-concurrent-edit-loss/notes.md) — restore
		// dirty so the human can fix the text and re-commit, or Escape to discard it.
		setDirty(true);
		commitError = error;
	}

	/** Escape: discards (no `Apply`, no op-log entry), reverting the buffer to `baseline`. */
	function discard() {
		if (!view || !dirty) return;
		view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: baseline } });
		setDirty(false);
		reportSave("synced");
		commitError = "";
	}

	async function refreshFilesByPath() {
		const files = await listFiles();
		filesByPath = new Map(files.map((f) => [f.path, f]));
		if (view) view.dispatch({ effects: lineDecoCompartment.reconfigure(lineDecorations(path, filesByPath)) });
	}

	// Bumped at the start of every `refreshDoc` call; a call's own number stops meaning "the latest
	// request" the moment a later call starts. Lets a stale/out-of-order response (two `Watch`
	// changes firing close together, or one `getFile` round trip simply finishing after a newer
	// one) detect that it's been superseded and skip repainting with old content.
	let refreshSeq = 0;

	/**
	 * Replaces the whole document with fresh `get_file` bytes — the Watch feed is "here's what
	 * changed," not a diff, so we never try to patch the doc from `Change.ops` ourselves
	 * (tasks/desktop-main-view/notes.md). Dispatching one full-range change (rather than
	 * `view.setState`) still lets CM6 map the viewport/decorations/scroll through the edit, so an
	 * in-place edit doesn't visually "reload" the file.
	 */
	async function refreshDoc() {
		if (!view) return;
		// A concurrent `Watch` change (another device, or the file watcher) must never silently
		// overwrite the human's in-progress edit. The conflict banner (a sibling component, fed by
		// the same `Watch` stream) still shows any `needs_review` flag the change raised; this
		// guard only holds off the *document* repaint until the dirty buffer has been committed or
		// discarded.
		if (dirty) return;
		const seq = ++refreshSeq;
		try {
			const contents = await getFile(path);
			// Re-check both guards: `dirty` can turn true, and a newer `refreshDoc` call can start
			// and finish, while this `getFile` round trip is in flight (root cause 2,
			// tasks/desktop-concurrent-edit-loss/notes.md) — either means this response is stale.
			if (!view || dirty || seq !== refreshSeq) return;
			loadError = "";
			baseline = contents.text;
			baselineHash = contents.hash;
			baselineTaskIds = contents.task_ids ?? [];
			baselineWorkspace = get(currentWorkspaceRoot);
			const current = view.state.doc.toString();
			if (current !== contents.text) {
				view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: contents.text } });
			}
			if (pendingCursor !== null) {
				placeCursor(pendingCursor);
				pendingCursor = null;
			}
		} catch (e) {
			if (seq !== refreshSeq) return; // a newer refresh already superseded this one
			loadError = String(e);
		}
	}

	let unlistenChange: (() => void) | undefined;

	// List mode (specs/client-parity.toml list.*; `$lib/todotxt/listMode.ts`): while nothing has
	// the keyboard, the caret's line is the current line. Moves set the caret; edits go to the
	// daemon, and the caret lands where the command says once its change repaints the list.
	let pendingCursor: number | null = null;

	/** Puts the caret at the start of `line` (clamped) and scrolls it into view, without focus. */
	function placeCursor(line: number) {
		if (!view) return;
		const n = Math.min(Math.max(line, 1), view.state.doc.lines);
		const at = view.state.doc.line(n).from;
		view.dispatch({ selection: { anchor: at }, effects: EditorView.scrollIntoView(at, { y: "nearest" }) });
	}

	async function runListCommand(id: string) {
		if (!view) return;
		const current = view.state.doc.lineAt(view.state.selection.main.head);
		const doc = { lines: docLines(baseline), taskIds: baselineTaskIds };
		const target = moveTarget(id, current.number, doc);
		if (target !== null) return placeCursor(target);
		if (id === "list.edit_start" || id === "list.edit_end") {
			const at = id === "list.edit_start" ? current.from : current.to;
			view.dispatch({ selection: { anchor: at } });
			return view.focus();
		}
		if (id === "detail.open") return requestDetail(current.number);
		if (id === "list.undo") return undoNewest();
		if (dirty) await commit();
		if (dirty) return; // the buffer was refused: its error is already showing
		const reopen = async (line: string) => (await applyChipCore(line, 0, "x", localToday()))?.text ?? line;
		const change = await listMutations(id, current.number, doc, localToday(), reopen);
		if (!change) return;
		try {
			pendingCursor = change.cursor;
			await applyMutations(path, change.mutations, baselineWorkspace);
			commitError = "";
			// Repaint now, not on the change event: the next key must see the new order.
			await refreshDoc();
			offerUndo(id, current.text);
		} catch (err) {
			pendingCursor = null;
			commitError = String(err);
		}
	}

	/** `u`: takes back this session's newest change, all its ops (`$lib/stores/changes.ts`). */
	async function undoNewest() {
		const change = popChange();
		if (!change) return void showToast("Nothing to undo from this session");
		try {
			await undoChange(change);
			await refreshDoc();
		} catch (err) {
			commitError = String(err);
		}
	}

	/** A toast after a completion or a delete, whose Undo takes back exactly that change while it
	 * is still the newest (toast.undo). */
	function offerUndo(id: string, line: string) {
		const words: Record<string, string> = {
			"list.toggle_complete": /^x /.test(line) ? "Reopened" : "Completed",
			"list.delete": "Deleted"
		};
		const change = newestChange();
		if (!words[id] || !change) return;
		showToast(words[id], () => {
			if (!popIfNewest(change)) return void showToast("A later change came first; press u to step back");
			void undoChange(change)
				.then(refreshDoc)
				.catch((err) => (commitError = String(err)));
		});
	}

	onMount(() => {
		// One command at a time: a key pressed while the last one is still landing waits for it,
		// so it acts on the list as it now is (a fast `J` then `x` used to hit the old line).
		// Every list takes list mode's keys while it is the active one (`$lib/listCommands.ts`): from
		// its mount (a sub-list mounts as its detail opens), and again on a click or focus inside it.
		let listQueue = Promise.resolve();
		const list = listBuffer(
			(id) => {
				listQueue = listQueue.then(() => runListCommand(id)).catch(() => {});
			},
			(active) => (activeList = active),
			() => path
		);
		// Capture phase, so the editor's own handlers cannot stop it first.
		// Ref: https://developer.mozilla.org/en-US/docs/Web/API/EventTarget/addEventListener#capture
		rootEl?.addEventListener("pointerdown", list.activate, true);
		rootEl?.addEventListener("focusin", list.activate, true);
		view = new EditorView({ state: EditorState.create({ doc: "", extensions: initialExtensions() }), parent: containerEl });
		lastStep = get(searchStep).n;
		// Matching runs in the wasm core: once it has loaded, apply whatever is already typed. Every
		// list marks the header search's hits (a detail's sub-list too); only the main buffer counts
		// them for the pill (task desktop-list-parity).
		void wasmReady().then(() => {
			view?.dispatch({ effects: setQuery.of(get(searchQuery)) });
			countHitsSoon();
		});

		let cancelled = false;
		(async () => {
			await refreshFilesByPath();
			await refreshDoc();
			await watch();
			const unlisten = await onDaemonChange((change) => {
				if (change.path === path) refreshDoc();
			});
			if (cancelled) unlisten();
			else unlistenChange = unlisten;
		})();

		return () => {
			cancelled = true;
			list.release();
		};
	});

	onDestroy(() => {
		unlistenChange?.();
		clearTimeout(savedTimer);
		if (reportStatus) editorStatus.set(EMPTY_STATUS);
		// Best-effort, fire-and-forget commit: a dirty buffer must never just vanish with the
		// component (an unmount going back to the root view is a "switch" too, same as the
		// `$effect` below). The `Apply` call outlives the component; nothing here awaits it, since
		// `onDestroy` can't block teardown.
		if (dirty && view) {
			// A refusal lands in `rejectedEdits` (the banner in MainView outlives this component).
			void commitOutgoing(path, { text: baseline, hash: baselineHash }, view.state.doc.toString());
		}
		clearTimeout(reorderTimer);
		view?.destroy();
	});

	// The header search drives the main buffer's search (task desktop-ui-revamp): the query goes in
	// as an effect, the hit count follows a moment later (it walks the whole document), and
	// Enter / Shift+Enter step through the hits without taking focus from the search field.
	let countTimer: ReturnType<typeof setTimeout> | undefined;
	function countHitsSoon() {
		if (!reportStatus) return;
		clearTimeout(countTimer);
		countTimer = setTimeout(() => {
			if (!view) return;
			const hits = hitLines(view.state).length;
			editorStatus.update((s) => ({ ...s, search: { hits, index: 0 } }));
		}, 60);
	}
	$effect(() => {
		const query = $searchQuery;
		if (!view) return;
		view.dispatch({ effects: setQuery.of(query) });
		countHitsSoon();
	});
	let lastStep = 0;
	$effect(() => {
		const step = $searchStep;
		if (!reportStatus || !view || step.n === lastStep) return;
		lastStep = step.n;
		const hit = stepHit(view, step.dir);
		editorStatus.update((s) => ({ ...s, search: hit ? { hits: hit.total, index: hit.index } : { hits: 0, index: 0 } }));
	});

	// Settings > Appearance can flip the gutter or the long-line hint while the buffer is open.
	$effect(() => {
		const { lineNumbers: numbers, lengthHint } = $editorPrefs;
		view?.dispatch({
			effects: [
				gutterCompartment.reconfigure(gutterFor(numbers)),
				lengthHintCompartment.reconfigure(lengthHintFor(lengthHint))
			]
		});
	});


	// A future detail view can swap `path` on a mounted FileView (e.g. selecting a different
	// sub-list) — re-baseline against the new file rather than assuming `path` is fixed for life.
	// svelte-ignore state_referenced_locally -- intentional: tracks path's *previous* value to
	// detect a change, not a live derivation of it.
	let mountedPath = path;
	$effect(() => {
		if (path !== mountedPath && view) {
			const oldPath = mountedPath;
			const newPath = path;
			mountedPath = path;

			// A dirty buffer is per-instance, per-file state — commit (or, if unchanged, no-op) the
			// outgoing file's buffer *before* this instance starts showing `newPath`, so it is
			// never carried across the switch. This mirrors `commit` but targets `oldPath`, since
			// by the time this effect runs `path` (and hence a plain `commit()` call) already means
			// the destination, not the file the buffer belongs to.
			const wasDirty = dirty;
			const oldBaseline = { text: baseline, hash: baselineHash };
			clearTimeout(reorderTimer);
			reorderTimer = undefined;
			const bufferAtSwitch = wasDirty && view ? view.state.doc.toString() : "";
			if (wasDirty) setDirty(false);
			// A refusal is parked in `rejectedEdits` (the buffer is about to be replaced by
			// `newPath`'s text) and shown on `commitError`, which `refreshDoc` below leaves alone.
			commitError = "";
			const settle = wasDirty
				? commitOutgoing(oldPath, oldBaseline, bufferAtSwitch).then((error) => {
						if (error !== null) commitError = error;
					})
				: Promise.resolve();

			settle.then(() => {
				if (!view) return;
				view.dispatch({ effects: lineDecoCompartment.reconfigure(lineDecorations(newPath, filesByPath)) });
				refreshDoc();
				watch();
			});
		}
	});
</script>

<div class="file-view" class:fill class:active-list={activeList} bind:this={rootEl} style={`--depth: ${depth};`} data-line-count={docLineCount}>
	{#if loadError}
		<p class="error" role="alert">{loadError}</p>
	{/if}
	{#if commitError}
		<p class="error" role="alert">{commitError}</p>
	{/if}

	<div class="editor-shell" bind:this={containerEl}></div>
</div>

<style>
	.file-view {
		position: relative;
		padding-left: calc(var(--depth, 0) * 1rem);
	}

	.file-view.fill {
		display: flex;
		flex-direction: column;
		flex: 1;
		min-height: 0;
		height: 100%;
	}

	.error {
		color: var(--color-danger);
		margin: 0 0.5rem 0.4rem;
		font-size: 0.85rem;
	}

	.file-view.fill .editor-shell {
		flex: 1;
		min-height: 0;
	}

	.editor-shell {
		border: 1px solid var(--color-border-subtle);
		border-radius: 6px;
		max-height: 70vh;
		overflow: auto;
	}

	/* Filling mode: no card chrome, no cap — the editor itself is the whole available area. */
	.file-view.fill .editor-shell {
		border: none;
		border-radius: 0;
		max-height: none;
		height: 100%;
	}

	/* Only the list that has list mode's keys marks its current line. */
	.file-view:not(.active-list) .editor-shell :global(.cm-activeLine),
	.file-view:not(.active-list) .editor-shell :global(.cm-activeLineGutter) {
		background: transparent;
	}

	.file-view.fill .editor-shell :global(.cm-editor) {
		height: 100%;
	}
</style>
