// Ref: https://vitest.dev/config/
// Pure-logic unit tests only (no Svelte component runtime under test), so plain Node — no jsdom/
// happy-dom dependency needed for the `apps/desktop/src/devices/*.test.ts` suite this configures.
import { defineConfig } from "vitest/config";

export default defineConfig({
	test: {
		include: ["src/**/*.test.ts"]
	}
});
