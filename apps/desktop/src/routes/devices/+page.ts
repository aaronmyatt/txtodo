// `/devices` became the Devices card of Settings (task desktop-ui-revamp). Kept for one release so
// an old link still lands; the redirect runs client-side (SPA mode, `ssr = false`).
// Ref: https://svelte.dev/docs/kit/load#Redirects
import { redirect } from "@sveltejs/kit";

export function load(): never {
	redirect(307, "/settings#devices");
}
