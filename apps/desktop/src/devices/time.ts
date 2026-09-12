// Pure relative-time formatting for the activity feed (`op_log()` is a one-shot fetch, not a live
// stream — see `api.ts` — so every row's time label is computed once at render/refresh time, not
// updated on a ticking clock).

const MINUTE_MS = 60_000;
const HOUR_MS = 60 * MINUTE_MS;
const DAY_MS = 24 * HOUR_MS;

/**
 * Formats `atMs` relative to `nowMs` as a short, human label. Always describes the past — the op
 * log has no future entries — and floors to whole units so "59s ago" doesn't flicker to "1m ago"
 * a moment later on a re-render at the same `nowMs`.
 */
export function relativeTime(atMs: number, nowMs: number = Date.now()): string {
	const deltaMs = Math.max(0, nowMs - atMs);
	if (deltaMs < MINUTE_MS) return "just now";
	if (deltaMs < HOUR_MS) return `${Math.floor(deltaMs / MINUTE_MS)}m ago`;
	if (deltaMs < DAY_MS) return `${Math.floor(deltaMs / HOUR_MS)}h ago`;
	if (deltaMs < 30 * DAY_MS) return `${Math.floor(deltaMs / DAY_MS)}d ago`;
	return new Date(atMs).toISOString().slice(0, 10);
}
