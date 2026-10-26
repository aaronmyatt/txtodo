// How a removed device row reads (task peer-prune, ADR 0044), from `Device.removed_reason`. A row
// the daemon did not say why of (an older daemon, or removed before it recorded why) reads as plain
// "removed", and a reason this build does not know does too: the wire value is external input.
import type { Device } from "./types";

// A Map, not an object: `"toString" in {}` is true, and the reason is external input.
const DROPPED = new Map<string, string>([
	["pruned_quiet", "Pruned: not heard from for too long."],
	["pruned_parked", "Pruned: its keys did not match ours for too long."],
	["left_out", "Dropped by another of your devices."]
]);

/** True for a device the group dropped (pruned, or dropped by another device this one followed),
 * not one a person removed. Only these get an explanation and a way back. */
export function wasDropped(d: Pick<Device, "removed" | "removed_reason">): boolean {
	return d.removed && DROPPED.has(d.removed_reason);
}

/** The line shown under a removed device's name; empty for a live one or one removed by hand. */
export function droppedNote(d: Pick<Device, "removed" | "removed_reason">): string {
	const why = d.removed ? DROPPED.get(d.removed_reason) : undefined;
	return why ? `${why} Pair it again to bring it back.` : "";
}
