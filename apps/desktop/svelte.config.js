// Tauri doesn't have a Node.js server to do proper SSR
// so we use adapter-static with a fallback to index.html to put the site in SPA mode
// See: https://svelte.dev/docs/kit/single-page-apps
// See: https://v2.tauri.app/start/frontend/sveltekit/ for more info
import adapter from "@sveltejs/adapter-static";
import { vitePreprocess } from "@sveltejs/vite-plugin-svelte";

/** @type {import('@sveltejs/kit').Config} */
const config = {
  preprocess: vitePreprocess(),
  kit: {
    // The browser build (`npm run build:web`, ADR 0042) writes to build-web/, so it never
    // overwrites build/, the folder Tauri packages. https://svelte.dev/docs/kit/adapter-static
    adapter: adapter({
      fallback: "index.html",
      pages: process.env.TXTODO_WEB_BUILD ? "build-web" : "build",
      assets: process.env.TXTODO_WEB_BUILD ? "build-web" : "build",
    }),
  },
};

export default config;
