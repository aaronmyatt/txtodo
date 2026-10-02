// The not-syncing banner's wording (task sync-divergence-check/protocol-mismatch).
// Ref (vitest): https://vitest.dev/api/
import { describe, expect, it } from "vitest";
import type { SyncPeer } from "$lib/daemon";
import { protocolBanner } from "./syncProtocol";

const peer = (device: string, their_protocol: number): SyncPeer => ({
	device,
	lag_ms: 0,
	parked: false,
	stuck: 0,
	their_protocol
});

describe("protocolBanner", () => {
	it("is null while every peer speaks our protocol", () => {
		expect(protocolBanner({ protocol: 2, peers: [peer("01J9K3H5Z7Q8X2M4N6P8R0T2V5", 0)] })).toBeNull();
	});

	it("names the peer, both protocols, and that this device is the older one", () => {
		const b = protocolBanner({ protocol: 2, peers: [peer("01J9K3H5Z7Q8X2M4N6P8R0T2V5", 3)] });
		expect(b).toEqual({
			label: "A paired device is not syncing",
			detail:
				"01J9K3H5Z7… speaks sync protocol 3, this device 2; this device is older: upgrade txtodo here."
		});
	});

	it("says to upgrade the other device when it is the older one, and counts several", () => {
		const b = protocolBanner({
			protocol: 3,
			peers: [peer("01J9K3H5Z7Q8X2M4N6P8R0T2V5", 2), peer("01J9K3H5Z7Q8X2M4N6P8R0T2V6", 2)]
		});
		expect(b?.label).toBe("2 paired devices are not syncing");
		expect(b?.detail.endsWith("upgrade txtodo on that device.")).toBe(true);
	});
});
