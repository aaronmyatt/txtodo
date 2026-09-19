// Playwright globalTeardown (tasks/test-registry-leak-cleanup): each spec's own
// `beforeEach`/`afterEach` already spawns and disposes a fresh, isolated daemon per test
// (fixtures.ts::spawnDaemon/dispose) — this is the suite-wide safety net for when that per-test
// cleanup itself never ran (a crashed/timed-out test, or dispose()'s own best-effort steps failing
// silently). Asserts (throws, failing the run) rather than silently absorbing a leak, so whichever
// spec skipped its own cleanup gets noticed and fixed at the source — best-effort cleanup still
// runs first so a human's local machine doesn't keep accumulating the 124-dead-row incident this
// task's notes.md documents.
//
// Reaps only what THIS run created (root todo id:01M2WK5DQQPMD3K6QP5YX9MTRD): tempdirs under this
// run's prefix and the `e2e_bridge` pids fixtures.ts recorded, never "every e2e_bridge on the
// machine", so a parallel run in another worktree keeps its daemons and its workspaces.
// Ref: https://playwright.dev/docs/test-global-setup-teardown
import { execFileSync } from "node:child_process";
import { readdirSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { bridgePidFile, tmpPrefix } from "./runId";

/** `pgrep`'s own exit code is 1 (not an error) when nothing matches; only macOS/Linux have it
 * (Windows CI, if this suite ever runs there, degrades to "found nothing" rather than crashing). */
function pgrep(args: string[]): number[] {
	try {
		return execFileSync("pgrep", args, { encoding: "utf8" })
			.trim()
			.split("\n")
			.filter(Boolean)
			.map(Number);
	} catch {
		return [];
	}
}

/** True only when `pid` is still alive AND still an `e2e_bridge` — a pid this run recorded that
 * the OS has since handed to an unrelated process must not be killed. */
function isLiveBridge(pid: number): boolean {
	try {
		return execFileSync("ps", ["-p", String(pid), "-o", "comm="], { encoding: "utf8" })
			.trim()
			.endsWith("e2e_bridge");
	} catch {
		return false; // `ps -p` exits 1 when the process is gone
	}
}

function recordedBridgePids(): number[] {
	try {
		return readFileSync(bridgePidFile(), "utf8").split("\n").filter(Boolean).map(Number);
	} catch {
		return []; // no spec spawned a bridge
	}
}

export default function globalTeardown(): void {
	const problems: string[] = [];
	const prefix = tmpPrefix();

	// The pid list is a file under this run's prefix, so it is swept with the dirs below — read it
	// first. Not itself a leak.
	const bridgePids = recordedBridgePids().filter(isLiveBridge);

	const leaked = readdirSync(tmpdir()).filter((name) => name.startsWith(prefix) && !name.endsWith("bridge-pids"));
	for (const name of readdirSync(tmpdir()).filter((n) => n.startsWith(prefix))) {
		try {
			rmSync(join(tmpdir(), name), { recursive: true, force: true });
		} catch {
			// best-effort cleanup only; still reported below
		}
	}
	if (leaked.length > 0) {
		problems.push(`${leaked.length} leaked tempdir(s): ${leaked.join(", ")}`);
	}

	// e2e_bridge is test-only (never runs outside this harness — see its own module doc), so any
	// instance of this run still alive once every spec has finished is unambiguously a leak, along
	// with the real txtodod it spawned as its own child (fixtures.ts::killDaemon reaps that pid
	// specifically; a bridge that never reached its own afterEach never ran that step).
	for (const pid of bridgePids) {
		for (const child of pgrep(["-P", String(pid)])) {
			try {
				process.kill(child, "SIGKILL");
			} catch {
				// already gone
			}
		}
		try {
			process.kill(pid, "SIGKILL");
		} catch {
			// already gone
		}
	}
	if (bridgePids.length > 0) {
		problems.push(`${bridgePids.length} leaked e2e_bridge process(es): ${bridgePids.join(", ")}`);
	}

	if (problems.length > 0) {
		throw new Error(
			"e2e teardown found leaks a per-test dispose() should already have caught " +
				"(tasks/test-registry-leak-cleanup) — cleaned up best-effort, but failing so the " +
				"specific spec that skipped its own cleanup gets fixed:\n" +
				problems.join("\n")
		);
	}
}
