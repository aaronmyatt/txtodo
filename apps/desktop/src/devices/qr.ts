// The pairing QR's wire format: JSON carrying exactly `PairOffer`'s five fields, byte for byte
// what `crates/txtodo-daemon/src/pairing_wire.rs`'s `code_to_offer` decodes on the other end (that
// module's doc literally says "the same text a frontend's JSON.stringify(pair_offer_response)
// produces for the QR"). This file is the one place that JSON gets built and parsed, so the
// security invariant from notes.md — "the QR must never encode key material... the decoded
// payload contains no field that isn't already public in PairOffer" — has one seam to test.
import type { PairOffer } from "./types";

/** `PairOffer`'s field names, in the order the daemon's own `response_to_code` writes them. */
const PAIR_OFFER_FIELDS = ["device", "group_id", "x25519_pub", "endpoint", "nonce"] as const;

/**
 * Copies exactly `PairOffer`'s five fields into a fresh object. Never spread `offer` verbatim —
 * an object-literal allow-list is the only way a field added upstream (say, a future private key)
 * can't silently ride along into a QR code.
 */
export function toQrPayload(offer: PairOffer): PairOffer {
	return {
		device: offer.device,
		group_id: offer.group_id,
		x25519_pub: offer.x25519_pub,
		endpoint: offer.endpoint,
		nonce: offer.nonce
	};
}

/** The exact text encoded into the QR code for a `pair_offer()` response. */
export function encodePairOfferQr(offer: PairOffer): string {
	return JSON.stringify(toQrPayload(offer));
}

/**
 * Parses a scanned QR's text back into a `PairOffer`, rejecting anything that isn't valid JSON,
 * is missing one of the five required fields, has a non-string value for one, or carries any
 * field beyond those five. Returns `null` rather than throwing — scanning is a loop over camera
 * frames, most of which decode to nothing meaningful, and a bad or malicious frame must never
 * reach `pair_accept()`.
 */
export function decodePairOfferQr(text: string): PairOffer | null {
	let parsed: unknown;
	try {
		parsed = JSON.parse(text);
	} catch {
		return null;
	}
	if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) return null;

	const keys = Object.keys(parsed as Record<string, unknown>).sort();
	const expected = [...PAIR_OFFER_FIELDS].sort();
	if (keys.length !== expected.length) return null;
	if (!keys.every((k, i) => k === expected[i])) return null;

	const record = parsed as Record<string, unknown>;
	for (const field of PAIR_OFFER_FIELDS) {
		if (typeof record[field] !== "string") return null;
	}
	return {
		device: record.device as string,
		group_id: record.group_id as string,
		x25519_pub: record.x25519_pub as string,
		endpoint: record.endpoint as string,
		nonce: record.nonce as string
	};
}
