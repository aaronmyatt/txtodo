import { describe, expect, it } from "vitest";
import type { SyncPeer } from "../daemon";
import { peerLines, syncSummary } from "./summary";

const peer = (over: Partial<SyncPeer>): SyncPeer => ({
	device: "01J9K3H5Z7Q8X2M4N6P8R0T2V5",
	lag_ms: 400,
	parked: false,
	stuck: 0,
	stuck_files: [],
	their_protocol: 0,
	...over
});

describe("syncSummary", () => {
	it("reads no peers, then counts, as the TUI does", () => {
		expect(syncSummary({ protocol: 2, peers: [], pending_ops: 0 })).toEqual({ tone: "ok", text: "no peers · 0 pending" });
		expect(syncSummary({ protocol: 2, peers: [peer({}), peer({ lag_ms: 900 })], pending_ops: 3 })).toEqual({
			tone: "busy",
			text: "2 peers · lag 900ms · 3 pending"
		});
	});

	it("goes bad on a stuck peer or another protocol, and says so in words", () => {
		const s = syncSummary({ protocol: 2, peers: [peer({ stuck: 1, stuck_files: ["todo.txt"] }), peer({ their_protocol: 3 })], pending_ops: 0 });
		expect(s).toEqual({ tone: "bad", text: "2 peers · lag 400ms · 0 pending · 1 stuck · 1 not syncing" });
	});
});

describe("peerLines", () => {
	it("lists lag, stuck files, parked and another protocol", () => {
		expect(peerLines(peer({ stuck_files: ["todo.txt"], parked: true, their_protocol: 3 }), 2)).toEqual([
			"01J9K3H5Z7…  lag 400ms",
			"stuck on todo.txt",
			"parked: no shared key",
			"not syncing: protocol 3 (this device 2)"
		]);
	});
});
