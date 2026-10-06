// Mock-only workspace offers for `npm run dev` (synthetic names and ids, never real ones).
import { NOW_MS } from "./state";

const offers = [
	{ offering_device: "01MOCKDEVICE00000000LAPTOP", workspace_id: "01MOCKWORKSPACE000000HOME1", name: "home", offered_at_ms: NOW_MS - 3_600_000 }
];

export function mockOffers() {
	return { offers: offers.map((o) => ({ ...o })), problem: "" };
}

/** Accept and decline both answer the offer: it leaves the list. */
export function mockAnswerOffer(args: Record<string, unknown>): boolean {
	const i = offers.findIndex((o) => o.offering_device === args.offeringDevice && o.workspace_id === args.workspaceId);
	if (i < 0) return false;
	offers.splice(i, 1);
	return true;
}
