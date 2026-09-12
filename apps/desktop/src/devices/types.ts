// Mirrors of the daemon's serde DTOs that cross the Tauri IPC bridge (design §7: the frontend
// never re-implements pairing/token/op-log logic, only renders what these commands return).
// Field sets here must stay byte-for-byte in sync with the Rust side — no extra fields, since the
// QR payload built from `PairOffer` is a security invariant (see `qr.ts`).
// Ref (DTO source of truth):
//   apps/desktop/src-tauri/src/dto_pairing.rs (PairOfferDto, PairResultDto)
//   apps/desktop/src-tauri/src/dto_tokens.rs  (TokenDto)
//   apps/desktop/src-tauri/src/dto_activity.rs (OpLogEntryDto)

/**
 * `pair_offer()`'s QR payload: identity + handshake material only, never a group key or a
 * private key. Mirrors `PairOfferDto` field for field (apps/desktop/src-tauri/src/dto_pairing.rs).
 */
export interface PairOffer {
	/** ULID text. */
	device: string;
	/** ULID text (decimal `u128` on the wire). */
	group_id: string;
	/** Hex-encoded X25519 public key. */
	x25519_pub: string;
	/** LAN transport address; empty until `sync-lan-transport` lands (plan M4). */
	endpoint: string;
	/** Hex-encoded handshake nonce. */
	nonce: string;
}

/**
 * The 6-word SAS (EFF short list), returned by both `pair_accept()` and `pair_confirm_sas()`.
 * Mirrors `PairResultDto`.
 */
export interface PairResult {
	/** Space-joined 6-word SAS. */
	sas: string;
}

/**
 * A single named, revocable capability token. Mirrors `TokenDto`
 * (apps/desktop/src-tauri/src/dto_tokens.rs). `secret` is non-empty only in `token_create()`'s own
 * response — `token_list()` always sends it back empty, never re-shown after creation.
 */
export interface Token {
	/** ULID text. */
	id: string;
	/** Human-chosen label. */
	name: string;
	/** Closed union (design §6.2): see {@link Scope}. */
	scopes: Scope[];
	/** RFC 3339; empty = no expiry. */
	expires: string;
	/** Unix ms. */
	created_at_ms: number;
	/** Bearer secret in plaintext; non-empty only right after creation. */
	secret: string;
}

/**
 * Design §6.2's closed scope grammar. The base scopes are fixed; `project:`/`context:`/`file:`
 * are restrictors that narrow every other scope to matching tasks and carry an arbitrary non-empty
 * suffix (a project/context name or a workspace-relative file path).
 */
export type Scope =
	| "read"
	| "write:add"
	| "write:complete"
	| "write:edit"
	| "write:delete"
	| "raw"
	| `project:${string}`
	| `context:${string}`
	| `file:${string}`;

/** The fixed (non-restrictor) scopes, in the order the create-token form renders them. */
export const BASE_SCOPES: readonly Scope[] = [
	"read",
	"write:add",
	"write:complete",
	"write:edit",
	"write:delete",
	"raw"
];

/** The three restrictor scope prefixes, each requiring a non-empty suffix. */
export const RESTRICTOR_PREFIXES = ["project", "context", "file"] as const;
export type RestrictorPrefix = (typeof RESTRICTOR_PREFIXES)[number];

/**
 * Runtime guard for the closed union above: the create-token form must never be able to send the
 * daemon a scope string it doesn't recognize (design §6.2 — "the daemon rejects an unrecognized
 * scope at create time", but the UI shouldn't rely on the daemon to catch a typo).
 */
export function isValidScope(s: string): s is Scope {
	if ((BASE_SCOPES as string[]).includes(s)) return true;
	for (const prefix of RESTRICTOR_PREFIXES) {
		if (s.startsWith(`${prefix}:`) && s.length > prefix.length + 1) return true;
	}
	return false;
}

/** Request body for `token_create(name, scopes, expires)`. Not a wire DTO itself — the command
 * takes these as separate positional args — but grouping them gives the form one typed shape to
 * build and validate before calling `invoke`. */
export interface TokenCreateReq {
	name: string;
	scopes: Scope[];
	/** RFC 3339; empty = no expiry. */
	expires: string;
}

/**
 * One `op_log()` row. Mirrors `OpLogEntryDto` (apps/desktop/src-tauri/src/dto_activity.rs) — the
 * same source `txtodo blame` reads.
 */
export interface OpEvent {
	/** `"you@dev"` / `"agent:name@dev"` / `"external@dev"`. */
	principal: string;
	/** One-line human summary of the mutation. */
	op: string;
	/** Unix ms. */
	at_ms: number;
}
