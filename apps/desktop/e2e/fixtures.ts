// Playwright harness: spawns a REAL `txtodod` on a seeded tempdir workspace, fronted by the
// test-only `e2e_bridge` HTTP bridge (apps/desktop/src-tauri/src/bin/e2e_bridge.rs), so the app
// under test drives real reconciliation/real file bytes/real op log, not a stubbed reducer
// (tasks/desktop-playwright-tests/notes.md). Each spec calls `spawnDaemon(fixture)` for its own
// fresh workspace — "no test may depend on another's side effects" (that file's own rule).
import { type ChildProcess, execFileSync, spawn } from "node:child_process";
import {
	appendFileSync,
	mkdirSync,
	mkdtempSync,
	readFileSync,
	realpathSync,
	rmSync,
	writeFileSync
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { bridgePidFile, tmpPrefix } from "./runId";
import { generateTenKLines, TEN_K_REF_SLUG } from "./tenKFixture";

const HERE = dirname(fileURLToPath(import.meta.url));
// apps/desktop/e2e -> apps/desktop -> apps -> repo root
const REPO_ROOT = join(HERE, "..", "..", "..");
const TARGET_DEBUG = join(REPO_ROOT, "target", "debug");

let builtOnce = false;

/** Builds `e2e_bridge` (behind its `e2e-bridge` feature — see that file's module doc for why it's
 * feature-gated) and `txtodod` once per Playwright run, not once per spec file. */
function ensureBuilt(): void {
	if (builtOnce) return;
	execFileSync(
		"cargo",
		["build", "-p", "desktop", "--features", "e2e-bridge", "--bin", "e2e_bridge"],
		{ cwd: REPO_ROOT, stdio: "inherit" }
	);
	execFileSync("cargo", ["build", "-p", "txtodo-daemon", "--bin", "txtodod"], {
		cwd: REPO_ROOT,
		stdio: "inherit"
	});
	builtOnce = true;
}

/** Playwright runs each test file in its own worker *process* by default, so a plain incrementing
 * counter here would restart at the same value in every worker and collide across parallel specs
 * — pick from a wide random range instead (fullyParallel: true in playwright.config.ts). */
function pickPort(): number {
	return 20_000 + Math.floor(Math.random() * 20_000);
}

export interface DaemonHandle {
	/** `e2e_bridge`'s HTTP port; pass this to `installBridgeUrl` for the page under test. */
	port: number;
	/** The tempdir workspace root — read files directly from here for a byte-diff assertion. */
	dir: string;
	/** Kills the bridge and the `txtodod` it spawned, then removes the tempdir. */
	dispose(): void;
}

/**
 * A named workspace layout each spec starts from. Fixtures that need a task to have a real,
 * client-resolvable `task_id` (e.g. to open the notes editor — see
 * `$lib/components/DetailView.svelte`'s guard) write a hand-authored `id:` tag: a workspace with
 * any `id:` tag on disk auto-detects `identity_mode = tagged`
 * (`crates/txtodo-daemon/src/workspace.rs`), which is what makes that resolvable at all under the
 * new sidecar-by-default identity mode (see tasks/desktop-detail-view/notes.md's "As built" for
 * the full explanation).
 */
export type FixtureName =
	| "todo"
	| "popover"
	| "nested"
	| "notes-create"
	| "conflict"
	| "ten-k"
	// A fresh profile: no workspace of its own, so the app opens the daemon's default one
	// (task default-workspace). `dir` is that default workspace's directory.
	| "fresh"
	// A workspace whose root list is `work.txt` (`txtodo.toml`, task workspace-layout): every
	// client must read the layout instead of assuming `todo.txt` (task layout-client-gaps).
	| "custom-root";

function seed(dir: string, fixture: FixtureName): void {
	switch (fixture) {
		case "todo":
			writeFileSync(join(dir, "todo.txt"), "(A) buy milk +home @errand\ncall mum @phone\n");
			return;
		case "popover":
			writeFileSync(join(dir, "todo.txt"), "(A) call mum id:01ARZ3NDEKTSV4RRFFQ69G5FAV\n");
			return;
		case "nested": {
			writeFileSync(
				join(dir, "todo.txt"),
				"(A) plan the roadmap ref:q4-roadmap id:01ARZ3NDEKTSV4RRFFQ69G5FA2\n"
			);
			// The default layout keeps the root list's ref dirs in `tasks/` (task workspace-layout).
			mkdirSync(join(dir, "tasks", "q4-roadmap"), { recursive: true });
			writeFileSync(join(dir, "tasks", "q4-roadmap", "todo.txt"), "(B) draft the outline\n");
			// Both files, as every ref in a real backlog has (task desktop-notes-hidden).
			writeFileSync(join(dir, "tasks", "q4-roadmap", "notes.md"), "Q4 goals: ship the outline first.\n");
			return;
		}
		case "notes-create":
			writeFileSync(
				join(dir, "todo.txt"),
				"(A) plan the roadmap id:01ARZ3NDEKTSV4RRFFQ69G5FA3\n"
			);
			return;
		case "conflict":
			// No priority prefix, matching `crates/txtodo-daemon/tests/grpc.rs`'s own
			// `resolve_merged_keeps_bytes_and_mine_writes_the_side_back` fixture exactly — see
			// `CONFLICT_MINE`/`CONFLICT_THEIRS`'s doc comment for why the shape matters here.
			writeFileSync(join(dir, "todo.txt"), `buy milk ${CONFLICT_ID_TAG}\n`);
			return;
		case "fresh":
			return; // nothing to seed: the daemon creates the default workspace itself
		case "custom-root":
			writeFileSync(join(dir, "txtodo.toml"), 'todo_file = "work.txt"\n');
			writeFileSync(join(dir, "work.txt"), "(A) plan the launch id:01ARZ3NDEKTSV4RRFFQ69G5FA5\n");
			return;
		case "ten-k":
			// tasks/desktop-visual-regression: the 10k-line fixture shared by the main-view snapshot
			// and the first-paint perf budget — see tenKFixture.ts's module doc for why it's a
			// generator, not a checked-in 10k-line text file. Line 1's `ref:` tag needs a real
			// sub-directory (not a dangling ref) so the main view's `n/m` progress decoration
			// resolves on the very first screen instead of silently rendering nothing
			// (`$lib/todotxt/lineInfo.ts::resolveRefIndicator` returns `null` for a dangling ref).
			writeFileSync(join(dir, "todo.txt"), generateTenKLines());
			mkdirSync(join(dir, "tasks", TEN_K_REF_SLUG), { recursive: true });
			writeFileSync(join(dir, "tasks", TEN_K_REF_SLUG, "todo.txt"), "x 2026-01-01 done sub-task\n(B) open sub-task\n");
			return;
	}
}

/** The ULID `id:` tag hand-seeded by the `"conflict"` fixture above — `conflict.spec.ts` needs it
 * to address `debugRaiseConflict`/`resolve`. */
export const CONFLICT_TASK_ID = "01ARZ3NDEKTSV4RRFFQ69G5FA4";
const CONFLICT_ID_TAG = `id:${CONFLICT_TASK_ID}`;

/**
 * `mine`/`theirs` for the `"conflict"` fixture, each the *whole description field including its
 * `id:` tag* — not just the human-readable words. `ReviewFlagDto`'s own doc comment says "this
 * device's description," which reads like it should exclude tags, but
 * `crates/txtodo-daemon/tests/grpc.rs`'s own `raise_flag` helper embeds the id tag inside `mine`/
 * `theirs` too (`format!("first task (mine) {id_text}")`) and resolving `mine`/`theirs` without it
 * fails server-side ("inserted line does not carry id ..." — confirmed by hand while writing this
 * spec). Match that exact shape rather than the doc comment's wording.
 */
export const CONFLICT_MINE = `buy milk ${CONFLICT_ID_TAG}`;
export const CONFLICT_THEIRS = `buy oat milk ${CONFLICT_ID_TAG}`;

/**
 * Raises a `needs_review` flag directly in the workspace's op-log store, standing in for the
 * daemon-to-daemon sync this scenario would otherwise need (see conflict.spec.ts's module doc and
 * `e2e_bridge.rs::cmd_debug_raise_conflict`'s doc comment for why: that transport isn't wired up
 * yet at all, tracked separately as `sync-loopback-converge`). Calls the bridge directly rather
 * than through the page, since this is test *setup*, not something the desktop UI itself does.
 */
export async function debugRaiseConflict(
	daemon: DaemonHandle,
	opts: { path: string; taskId: string; mine: string; theirs: string }
): Promise<void> {
	const res = await fetch(`http://127.0.0.1:${daemon.port}/invoke`, {
		method: "POST",
		headers: { "content-type": "application/json" },
		body: JSON.stringify({ cmd: "debug_raise_conflict", args: opts })
	});
	if (!res.ok) {
		throw new Error(`debug_raise_conflict failed: ${await res.text()}`);
	}
}

/** Spawns a fresh `e2e_bridge` (and, through it, a fresh `txtodod`) on a new tempdir seeded per
 * `fixture`. Waits for `/health` before returning, so the caller's first `page.goto` never races
 * the daemon's startup adoption of the seeded file. */
export async function spawnDaemon(fixture: FixtureName): Promise<DaemonHandle> {
	ensureBuilt();
	const fresh = fixture === "fresh";
	const dir = fresh ? "" : mkdtempSync(join(tmpdir(), tmpPrefix()));
	if (!fresh) seed(dir, fixture);
	const port = pickPort();

	// A SEPARATE tempdir for this fixture's isolated global-mode daemon state (socket, registry.db,
	// identity.db, pidfile, logs — see `e2e_bridge.rs`'s module doc). Deliberately not nested under
	// `dir`: `notes-create.spec.ts`'s negative assertion checks the workspace root's own top-level
	// listing is exactly `{todo.txt, .txtodo}` before any user action, so anything the daemon's
	// *global* state creates must live outside the workspace tree entirely, not just outside
	// `.txtodo/`.
	// `realpathSync`: macOS tmp dirs sit behind a `/var` -> `/private/var` symlink, and the daemon
	// reports a workspace by its canonical root, so a spec comparing `dir` to it needs the same form.
	const globalDir = realpathSync(mkdtempSync(join(tmpdir(), tmpPrefix("global-"))));
	// A fresh profile's workspace is the default one, which the daemon makes beside its socket.
	const workspaceDir = fresh ? join(globalDir, "default") : dir;

	const proc: ChildProcess = spawn(join(TARGET_DEBUG, "e2e_bridge"), [], {
		env: {
			...process.env,
			// `DesktopConfig.daemon_bin` defaults to `None` -> resolve `txtodod` on PATH
			// (apps/desktop/src-tauri/src/config.rs) — prepending target/debug here is simpler
			// than adding a bridge-only env override for a binary that already resolves via PATH.
			PATH: `${TARGET_DEBUG}:${process.env.PATH ?? ""}`,
			...(fresh ? {} : { TXTODO_WORKSPACE: dir }),
			E2E_BRIDGE_PORT: String(port),
			TXTODO_E2E_GLOBAL_DIR: globalDir
		},
		stdio: "ignore"
	});
	// globalTeardown reaps only the bridges this run recorded here (runId.ts), never every
	// `e2e_bridge` on the machine.
	if (proc.pid !== undefined) appendFileSync(bridgePidFile(), `${proc.pid}\n`);

	await waitForHealth(port);

	return {
		port,
		dir: workspaceDir,
		dispose() {
			killDaemon(globalDir);
			proc.kill("SIGKILL");
			try {
				if (!fresh) rmSync(dir, { recursive: true, force: true });
			} catch {
				// best-effort cleanup only
			}
			try {
				rmSync(globalDir, { recursive: true, force: true });
			} catch {
				// best-effort cleanup only
			}
		}
	};
}

async function waitForHealth(port: number, timeoutMs = 30_000): Promise<void> {
	const start = Date.now();
	while (Date.now() - start < timeoutMs) {
		try {
			const res = await fetch(`http://127.0.0.1:${port}/health`);
			if (res.ok) return;
		} catch {
			// bridge/daemon not listening yet
		}
		await new Promise((resolve) => setTimeout(resolve, 100));
	}
	throw new Error(`e2e_bridge on port ${port} never became healthy within ${timeoutMs}ms`);
}

/** Best-effort: reads the daemon's own pidfile (under `globalDir`, the isolated global-mode state
 * dir `TXTODO_E2E_GLOBAL_DIR` points `e2e_bridge.rs` at) and SIGKILLs it, since killing the bridge
 * process alone does not reap the `txtodod` child it spawned. `e2e_bridge.rs`'s `main` points
 * `DesktopConfig.global_socket_override`/`global_registry_override` at `globalDir` (not the real,
 * machine-global socket/registry.db `ensure_daemon` would otherwise bind) so every fixture's
 * `txtodod` is fully isolated — its pid file lands alongside that socket, per
 * `workspace_registry_paths.rs`'s "pid lock ... alongside it" rule, not under `<dir>/.txtodo/`
 * (that path only ever held a pidfile in the pre-ADR-0025, one-daemon-per-workspace model). */
function killDaemon(globalDir: string): void {
	try {
		const pid = readFileSync(join(globalDir, "txtodod.pid"), "utf8").trim();
		if (pid) process.kill(Number(pid), "SIGKILL");
	} catch {
		// no pidfile yet, or the process is already gone
	}
}
