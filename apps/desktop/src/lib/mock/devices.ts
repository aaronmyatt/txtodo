// Mock-only paired devices for `npm run dev` (synthetic names, never real devices or keys).
import { NOW_MS } from "./state";

const devices = [
	{ id: "01MOCKDEVICE0000000000SELF", name: "This Mac", is_self: true, removed: false, paired_at_ms: NOW_MS - 9e8, last_seen_ms: NOW_MS, own_device: true, removed_reason: "", removed_by: "", removed_at_ms: 0 },
	{ id: "01MOCKDEVICE00000000LAPTOP", name: "mara-laptop", is_self: false, removed: false, paired_at_ms: NOW_MS - 6e8, last_seen_ms: NOW_MS - 120_000, own_device: false, removed_reason: "", removed_by: "", removed_at_ms: 0 },
	{ id: "01MOCKDEVICE000000000PHONE", name: "pixel-9", is_self: false, removed: false, paired_at_ms: NOW_MS - 3e8, last_seen_ms: NOW_MS - 86_400_000, own_device: true, removed_reason: "", removed_by: "", removed_at_ms: 0 }
];

export function mockDevices() {
	return devices.map((d) => ({ ...d }));
}

export function mockDeviceRemove(args: Record<string, unknown>) {
	const d = devices.find((x) => x.id === args.id);
	if (!d) return { removed: false, already_removed: false, rotated_to_epoch: 0, message: "" };
	if (d.is_self) throw new Error("mock daemon: cannot remove this device");
	const already = d.removed;
	d.removed = true;
	d.removed_reason = "manual";
	d.removed_by = devices[0].id;
	d.removed_at_ms = Date.now();
	return {
		removed: true,
		already_removed: already,
		rotated_to_epoch: already ? 0 : 2,
		message: "Removed. It keeps what it already synced; new changes stop reaching it."
	};
}
