// A paired device on another sync protocol (task sync-divergence-check/protocol-mismatch): the two
// devices refuse each other and nothing syncs until the older one is upgraded. Pure, so the
// wording is unit-tested without a daemon (syncProtocol.test.ts); `ProtocolMismatchBanner.svelte`
// draws it.
import type { SyncStatus } from "$lib/daemon";

/** What the banner says, or `null` when every peer speaks our protocol. */
export interface ProtocolBanner {
	label: string;
	detail: string;
}

export function protocolBanner(status: SyncStatus): ProtocolBanner | null {
	const off = status.peers.filter((p) => p.their_protocol !== 0);
	const first = off[0];
	if (!first) return null;
	const label =
		off.length === 1
			? "A paired device is not syncing"
			: `${off.length} paired devices are not syncing`;
	const older =
		first.their_protocol > status.protocol
			? "this device is older: upgrade txtodo here"
			: "upgrade txtodo on that device";
	const short = first.device.slice(0, 10);
	const detail = `${short}… speaks sync protocol ${first.their_protocol}, this device ${status.protocol}; ${older}.`;
	return { label, detail };
}
