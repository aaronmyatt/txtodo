// Playwright globalSetup (root todo id:01M2WK5DQQPMD3K6QP5YX9MTRD): mints this run's id so its
// tempdirs and `e2e_bridge` pids can be told apart from another run's — see runId.ts and
// globalTeardown.ts.
// Ref: https://playwright.dev/docs/test-global-setup-teardown
import { randomBytes } from "node:crypto";
import { RUN_ID_ENV } from "./runId";

export default function globalSetup(): void {
	process.env[RUN_ID_ENV] = randomBytes(4).toString("hex");
}
