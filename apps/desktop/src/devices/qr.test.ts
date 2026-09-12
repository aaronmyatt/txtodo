// Security-invariant test (tasks/desktop-devices-screen/notes.md): the QR payload built from a
// `PairOffer` must hold *exactly* {device, group_id, x25519_pub, endpoint, nonce} — never more,
// never less, never key material beyond what the daemon's own PairOfferDto already carries.
import { describe, expect, it } from "vitest";
import { decodePairOfferQr, encodePairOfferQr, toQrPayload } from "./qr";
import type { PairOffer } from "./types";

const offer: PairOffer = {
	device: "01J9K3ZZZZZZZZZZZZZZZZZZZZ",
	group_id: "123456789",
	x25519_pub: "deadbeef",
	endpoint: "",
	nonce: "cafef00d"
};

describe("toQrPayload", () => {
	it("carries exactly PairOffer's five fields, nothing extra", () => {
		const payload = toQrPayload(offer);
		expect(Object.keys(payload).sort()).toEqual(
			["device", "endpoint", "group_id", "nonce", "x25519_pub"].sort()
		);
		expect(payload).toEqual(offer);
	});

	it("drops any extra field an upstream caller might add", () => {
		const withExtra = { ...offer, group_key: "TOP-SECRET" } as PairOffer & { group_key: string };
		const payload = toQrPayload(withExtra);
		expect(payload).not.toHaveProperty("group_key");
		expect(Object.keys(payload)).toHaveLength(5);
	});
});

describe("encodePairOfferQr / decodePairOfferQr", () => {
	it("round-trips a valid offer", () => {
		const text = encodePairOfferQr(offer);
		expect(decodePairOfferQr(text)).toEqual(offer);
	});

	it("produces JSON with exactly the five expected keys", () => {
		const text = encodePairOfferQr(offer);
		const parsed = JSON.parse(text);
		expect(Object.keys(parsed).sort()).toEqual(
			["device", "endpoint", "group_id", "nonce", "x25519_pub"].sort()
		);
	});

	it("rejects a payload with an extra field (e.g. smuggled key material)", () => {
		const malicious = JSON.stringify({ ...offer, x25519_priv: "hunter2" });
		expect(decodePairOfferQr(malicious)).toBeNull();
	});

	it("rejects a payload missing a required field", () => {
		const { nonce: _nonce, ...incomplete } = offer;
		expect(decodePairOfferQr(JSON.stringify(incomplete))).toBeNull();
	});

	it("rejects a field with the wrong type", () => {
		const wrongType = JSON.stringify({ ...offer, group_id: 123456789 });
		expect(decodePairOfferQr(wrongType)).toBeNull();
	});

	it("rejects non-JSON text", () => {
		expect(decodePairOfferQr("not json at all")).toBeNull();
	});

	it("rejects a JSON array or primitive", () => {
		expect(decodePairOfferQr("[1,2,3]")).toBeNull();
		expect(decodePairOfferQr('"just a string"')).toBeNull();
	});
});
