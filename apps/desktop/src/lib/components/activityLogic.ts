// Pure logic for `settings/ActivityCard.svelte` (task desktop-activity-cross-workspace) — no DOM, no Tauri,
// so it is plain-function testable, the same split `editPopoverLogic.ts` uses for its component.

/** Pure string check (notes.md: "no new backend signal required") — `principal` is already
 * formatted `"you@dev"` / `"agent:name@dev"` / `"external@dev"` by the daemon. */
export function isAgent(principal: string): boolean {
	return principal.startsWith("agent:");
}

/** Last path segment only, for the row's workspace chip — the full root is still available via
 * the row's own `title`. Falls back to `root` itself for a root with no `/` at all (e.g. `"C:"`
 * on a platform this app doesn't target today, or a test fixture's relative path). */
export function shortRoot(root: string): string {
	const parts = root.split("/").filter(Boolean);
	return parts[parts.length - 1] ?? root;
}

/** Who did it, as one glyph for the row (task desktop-ui-revamp): you, an agent, the sync from
 * another device, or an edit made outside txtodo. */
export type PrincipalKind = "you" | "agent" | "external";

export function principalKind(principal: string): PrincipalKind {
	if (isAgent(principal)) return "agent";
	if (principal.startsWith("external")) return "external";
	return "you";
}
