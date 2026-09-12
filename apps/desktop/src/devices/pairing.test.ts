// Asserts the UI surfaces the daemon's *real* pairing bounds (crates/txtodo-sync/src/
// nonce_registry.rs) rather than a guessed number, and classifies its refusal errors correctly so
// "pairing window closed" / "too many pairings" are never a silent no-op (notes.md).
import { describe, expect, it } from "vitest";
import {
	MAX_CONCURRENT_PAIRINGS,
	PAIRING_WINDOW_MS,
	SAS_WORD_COUNT,
	classifyPairingError,
	pairingWindowRemainingMs
} from "./pairing";

describe("pairing constants", () => {
	it("match crates/txtodo-sync/src/nonce_registry.rs and sas.rs exactly", () => {
		expect(PAIRING_WINDOW_MS).toBe(120_000);
		expect(MAX_CONCURRENT_PAIRINGS).toBe(1);
		expect(SAS_WORD_COUNT).toBe(6);
	});
});

describe("pairingWindowRemainingMs", () => {
	it("counts down from the full window", () => {
		expect(pairingWindowRemainingMs(1000, 1000)).toBe(PAIRING_WINDOW_MS);
		expect(pairingWindowRemainingMs(1000, 1000 + 30_000)).toBe(PAIRING_WINDOW_MS - 30_000);
	});

	it("clamps to zero, never negative", () => {
		expect(pairingWindowRemainingMs(1000, 1000 + PAIRING_WINDOW_MS + 60_000)).toBe(0);
	});
});

describe("classifyPairingError", () => {
	it("recognizes the too-many-open message", () => {
		expect(classifyPairingError("1 pairing(s) already open on this daemon")).toBe(
			"too-many-pairings"
		);
	});

	it("recognizes the window-expired message", () => {
		expect(classifyPairingError("the pairing window (120000 ms) has expired")).toBe(
			"window-expired"
		);
	});

	it("is case-insensitive and tolerates status-code wrapping", () => {
		expect(
			classifyPairingError('status: ResourceExhausted, message: "1 pairing(s) ALREADY OPEN on this daemon"')
		).toBe("too-many-pairings");
	});

	it("falls back to other for anything else", () => {
		expect(classifyPairingError("daemon not connected")).toBe("other");
	});
});
