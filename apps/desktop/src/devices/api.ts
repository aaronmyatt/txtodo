// Thin typed wrappers around the six Tauri commands this screen calls (same pattern as
// `$lib/daemon.ts`): no `fs`, no `path`, no raw socket/fetch/WebSocket — every daemon read or
// write crosses through `invoke` (design §7). No crypto, no pairing/token state lives here; the
// daemon is the only source of truth.
// Ref: https://v2.tauri.app/develop/calling-rust/
import { invoke } from "$lib/tauriShim";
import type { OpEvent, PairOffer, PairResult, Scope, Token } from "./types";

/** Starts a pairing handshake on this device; returns the QR payload. */
export function pairOffer(): Promise<PairOffer> {
	return invoke("pair_offer");
}

/** Accepts a peer's scanned QR text and begins the handshake; returns the 6-word SAS. */
export function pairAccept(code: string): Promise<PairResult> {
	return invoke("pair_accept", { code });
}

/**
 * Confirms the SAS shown to the human on this device. Call only after an explicit user tap.
 * `ownDevice`: "is the other device your own?" — the default list merges only when both say yes.
 */
export function pairConfirmSas(ownDevice: boolean): Promise<PairResult> {
	return invoke("pair_confirm_sas", { ownDevice });
}

/** Mints a new capability token. `expires` is RFC 3339 text; empty means no expiry. */
export function tokenCreate(name: string, scopes: Scope[], expires: string): Promise<Token> {
	return invoke("token_create", { name, scopes, expires });
}

/** Tokens for this workspace, scopes included; `secret` always comes back empty here. */
export function tokenList(): Promise<Token[]> {
	return invoke("token_list");
}

/** Revokes a token by id; the daemon refuses it on its next use. */
export function tokenRevoke(id: string): Promise<boolean> {
	return invoke("token_revoke", { id });
}

/**
 * Newest ops across every tracked file, at most ~200, newest first. A one-shot bounded fetch, not
 * a live stream — call again (a refresh button, or on focus) to see anything new.
 */
export function opLog(): Promise<OpEvent[]> {
	return invoke("op_log");
}
