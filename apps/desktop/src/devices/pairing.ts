// Pairing bounds mirrored from the daemon so the UI surfaces the *real* window/cap as explicit
// states, never a guessed number or a silent no-op (tasks/desktop-devices-screen/notes.md
// "Edge cases & invariants"). The daemon owns these values; this file only copies them for display
// — it never enforces them itself, the daemon always has the last word.
// Ref: crates/txtodo-sync/src/nonce_registry.rs

/** How long a `pair_offer()` QR stays valid before the daemon clears it. */
export const PAIRING_WINDOW_MS = 120_000;

/** How many pairing attempts this daemon allows open at once. */
export const MAX_CONCURRENT_PAIRINGS = 1;

/** Words in the SAS (Short Authentication String) the human compares across both devices. */
export const SAS_WORD_COUNT = 6;

/**
 * Milliseconds left in the pairing window that started at `openedAtMs`, clamped to 0. The Tauri
 * commands don't return `opened_at_ms` (see `PairOffer`), so the caller must stamp it itself right
 * after `pair_offer()` resolves — for a same-process Tauri IPC round trip the skew is negligible.
 */
export function pairingWindowRemainingMs(openedAtMs: number, nowMs: number): number {
	return Math.max(0, PAIRING_WINDOW_MS - (nowMs - openedAtMs));
}

/**
 * Classifies a daemon error string into the two named refusal states notes.md requires as visible
 * UI, or `"other"` for anything else. The daemon's `Display` text
 * (crates/txtodo-daemon/src/pairing_state.rs) is matched by substring rather than parsed exactly,
 * since it crosses a tonic `Status` and then a Tauri `Result<_, String>` — the exact wrapping
 * around the message is not part of the contract, only the wording of the message itself.
 */
export function classifyPairingError(message: string): "too-many-pairings" | "window-expired" | "other" {
	const m = message.toLowerCase();
	if (m.includes("already open")) return "too-many-pairings";
	if (m.includes("pairing window") || m.includes("has expired")) return "window-expired";
	return "other";
}
