import { fileURLToPath, URL } from "node:url";
import { defineConfig } from "vite";
import { sveltekit } from "@sveltejs/kit/vite";
// `@types/node` is now present (pulled in as a `vitest` peer devDependency for
// `src/devices/*.test.ts`), so `node:process` no longer needs a `@ts-expect-error` suppression.
import process from "node:process";
const host = process.env.TAURI_DEV_HOST;

// https://vite.dev/config/
export default defineConfig(({ mode }) => ({
  plugins: [sveltekit()],

  // `vite dev --mode e2e` (apps/desktop/playwright.config.ts's webServer) swaps the real
  // `@tauri-apps/api/*` modules for the test-only HTTP-bridge shims under e2e/shim/ — see that
  // directory's module docs. Every other mode (plain `npm run dev`, `vitest`, `tauri dev`) is
  // untouched: this alias only exists when `mode === "e2e"`.
  resolve:
    mode === "e2e"
      ? {
          alias: {
            "@tauri-apps/api/core": fileURLToPath(new URL("./e2e/shim/core.ts", import.meta.url)),
            "@tauri-apps/api/event": fileURLToPath(new URL("./e2e/shim/event.ts", import.meta.url)),
            "@tauri-apps/api/window": fileURLToPath(new URL("./e2e/shim/window.ts", import.meta.url)),
          },
        }
      : {},

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || "127.0.0.1",
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },
}));
