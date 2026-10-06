// The sync indicator's words (the TUI's `s`, crates/txtodo-tui/src/ui/sync.rs): one summary line
// and a few rows per peer. Tone signals convergence (ok: nothing pending; busy: ops in flight;
// bad: a peer stuck or on another protocol) but the text always states the counts, so colour is
// never the only signal.
import type { SyncPeer, SyncStatus } from "../daemon";

export type SyncTone = "ok" | "busy" | "bad";

export function syncSummary(s: SyncStatus): { tone: SyncTone; text: string } {
	const stuck = s.peers.reduce((n, p) => n + p.stuck, 0);
	const off = s.peers.filter((p) => p.their_protocol !== 0).length;
	const tone: SyncTone = stuck > 0 || off > 0 ? "bad" : s.pending_ops === 0 ? "ok" : "busy";
	const parts: string[] = [];
	if (s.peers.length === 0) parts.push("no peers");
	else {
		const lag = Math.max(...s.peers.map((p) => p.lag_ms));
		parts.push(`${s.peers.length} peer${s.peers.length === 1 ? "" : "s"} · lag ${lag}ms`);
	}
	parts.push(`${s.pending_ops} pending`);
	if (stuck > 0) parts.push(`${stuck} stuck`);
	if (off > 0) parts.push(`${off} not syncing`);
	return { tone, text: parts.join(" · ") };
}

/** One peer's lines for the popup: its lag, each stuck file, parked, another protocol. */
export function peerLines(p: SyncPeer, ours: number): string[] {
	const lines = [`${p.device.slice(0, 10)}…  lag ${p.lag_ms}ms`];
	for (const f of p.stuck_files) lines.push(`stuck on ${f}`);
	if (p.parked) lines.push("parked: no shared key");
	if (p.their_protocol !== 0) lines.push(`not syncing: protocol ${p.their_protocol} (this device ${ours})`);
	return lines;
}
