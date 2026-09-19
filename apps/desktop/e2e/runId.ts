// One id per Playwright run (root todo id:01M2WK5DQQPMD3K6QP5YX9MTRD). globalSetup mints it into
// `process.env`, which the runner's workers inherit, and everything this run creates — tempdirs and
// the pid list of its `e2e_bridge` processes — carries it. globalTeardown then reaps exactly those,
// so a second run in another worktree (or a `tauri dev` a human has open) is never touched.
// Ref: https://playwright.dev/docs/test-global-setup-teardown#option-2-configure-globalsetup-and-globalteardown
import { tmpdir } from "node:os";
import { join } from "node:path";

export const RUN_ID_ENV = "TXTODO_E2E_RUN";

export function runId(): string {
	const id = process.env[RUN_ID_ENV];
	if (!id) {
		throw new Error(`${RUN_ID_ENV} is unset: playwright.config.ts's globalSetup mints it, so run through that config`);
	}
	return id;
}

/** Prefix of every tempdir this run creates; `kind` tells them apart when listing a leak. */
export function tmpPrefix(kind = ""): string {
	return `txtodo-e2e-${runId()}-${kind}`;
}

/** Where fixtures append the pid of each `e2e_bridge` they spawn, one per line. Lives in the
 * tempdir under this run's prefix, so teardown's directory sweep removes it as well. */
export function bridgePidFile(): string {
	return join(tmpdir(), `${tmpPrefix()}bridge-pids`);
}
