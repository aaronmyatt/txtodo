// Playwright globalSetup (root todo id:01M2WK5DQQPMD3K6QP5YX9MTRD): mints this run's id so its
// tempdirs and `e2e_bridge` pids can be told apart from another run's — see runId.ts and
// globalTeardown.ts.
// Ref: https://playwright.dev/docs/test-global-setup-teardown
import { randomBytes } from "node:crypto";
import { ensureBuilt, PREBUILT_ENV } from "./fixtures";
import { RUN_ID_ENV } from "./runId";

export default function globalSetup(): void {
	process.env[RUN_ID_ENV] = randomBytes(4).toString("hex");
	// Build once, here, before any worker exists: globalSetup is not bound by a test's timeout, and
	// the env flag tells every worker's `spawnDaemon` to skip its own build.
	// Ref: https://playwright.dev/docs/test-global-setup-teardown#option-2-configure-globalsetup-and-globalteardown
	ensureBuilt();
	process.env[PREBUILT_ENV] = "1";
}
