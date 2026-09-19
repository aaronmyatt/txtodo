// tasks/desktop-playwright-tests: structural/DOM assertions against the real app + a real
// txtodod (via the test-only e2e_bridge, see e2e/fixtures.ts and e2e/shim/*.ts). Also hosts
// tasks/desktop-visual-regression's two theme projects (light/dark, `e2e/visual/**`) and the perf
// budget (`e2e/perf.spec.ts`) — same harness, same daemon, different assertions.
// Ref: https://playwright.dev/docs/test-configuration
import { defineConfig } from "@playwright/test";

// Distinct from tauri dev's fixed 1420 and vite preview's default 4173, so a Playwright run never
// collides with either a `tauri dev` or `vite preview` a human happens to have open locally.
const PORT = 4373;

export default defineConfig({
	testDir: "./e2e",
	timeout: 30_000,
	// tasks/test-registry-leak-cleanup: suite-wide safety net behind every spec's own per-test
	// dispose() — see globalTeardown.ts's own doc.
	globalTeardown: "./e2e/globalTeardown.ts",
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
	},
	// No silent golden regeneration in CI (desktop-visual-regression/notes.md): this file's own
	// default (no `--update-snapshots` flag) already makes `toHaveScreenshot` fail on drift instead
	// of overwriting; regenerating goldens is only ever `npx playwright test e2e/visual --update-snapshots`,
	// run and reviewed by a human, never invoked automatically by the gate or CI.
	//
	// tasks/desktop-visual-regression/notes.md sketches "one project per theme" covering every
	// spec; scoped by `testMatch` here instead so the six functional scenarios and the perf test —
	// none of which call `toHaveScreenshot` or care about `colorScheme` — run exactly once each
	// rather than once per theme.
	// Regexes, not glob strings: Playwright expands a bare (no-slash) glob string like
	// "*.spec.ts" to match at ANY depth (confirmed the hard way — it matched e2e/visual/**
	// too), which would have run every visual spec three times over. These anchor on the actual
	// path instead.
	projects: [
		{
			name: "functional",
			testMatch: /\/e2e\/[^/]+\.spec\.ts$/ // e2e/*.spec.ts only — not e2e/visual/**.
		},
		{
			name: "light",
			testMatch: /\/e2e\/visual\/.+\.spec\.ts$/,
			use: { colorScheme: "light" }
		},
		{
			name: "dark",
			testMatch: /\/e2e\/visual\/.+\.spec\.ts$/,
			use: { colorScheme: "dark" }
		}
	]
});
