// tasks/desktop-playwright-tests: structural/DOM assertions against the real app + a real
// txtodod (via the test-only e2e_bridge, see e2e/fixtures.ts and e2e/shim/*.ts) — never a
// screenshot/visual-diff comparison (that's the separate, out-of-scope desktop-visual-regression
// task). Ref: https://playwright.dev/docs/test-configuration
import { defineConfig } from "@playwright/test";

// Distinct from tauri dev's fixed 1420 and vite preview's default 4173, so a Playwright run never
// collides with either a `tauri dev` or `vite preview` a human happens to have open locally.
const PORT = 4373;

export default defineConfig({
	testDir: "./e2e",
	timeout: 30_000,
	// Each spec spawns its own daemon + e2e_bridge (tasks/desktop-playwright-tests/notes.md: "no
	// test may depend on another's side effects") on a freely-chosen port from `fixtures.ts`'s
	// counter; running specs in parallel workers is fine; the shared dev-server webServer is not
	// what's being isolated here.
	fullyParallel: true,
	use: {
		baseURL: `http://127.0.0.1:${PORT}`
	},
	webServer: {
		command: `npm run dev -- --mode e2e --port ${PORT} --strictPort`,
		port: PORT,
		reuseExistingServer: !process.env.CI,
		timeout: 30_000
	}
});
