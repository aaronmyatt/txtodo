// Whether this is the browser build (`vite --mode web`, ADR 0042). Vite fixes `MODE` at build
// time, so the desktop bundle carries `false` and drops the web-only branches.
// Ref: https://vite.dev/guide/env-and-mode.html#modes

/** True in the browser build only. */
export const IS_WEB = import.meta.env.MODE === "web";

/** Settings sections a browser has nothing behind yet: no folders, no daemon tokens, no pairing
 * (the web-pwa Sync line), no device-wide activity log. */
export const WEB_HIDDEN_SECTIONS = new Set(["workspaces", "devices", "tokens", "activity"]);
