import { describe, expect, it } from "vitest";
import { listBuffer, onListCommand, runListCommand } from "./listCommands";

describe("list commands", () => {
	it("reach every listener but only the active list", () => {
		const heard: string[] = [];
		const offSync = onListCommand((id) => heard.push(`sync:${id}`));
		const root = listBuffer((id) => heard.push(`root:${id}`));
		runListCommand("list.down");
		// A sub-list mounting (its detail opened) takes the keys.
		const sub = listBuffer((id) => heard.push(`sub:${id}`));
		runListCommand("list.toggle_complete");
		// A click on the root list takes them back; closing the detail leaves them there.
		root.activate();
		runListCommand("list.up");
		sub.release();
		runListCommand("list.delete");
		root.release();
		offSync();
		expect(runListCommand("list.down")).toBe(false);
		expect(heard).toEqual([
			"sync:list.down",
			"root:list.down",
			"sync:list.toggle_complete",
			"sub:list.toggle_complete",
			"sync:list.up",
			"root:list.up",
			"sync:list.delete",
			"root:list.delete"
		]);
	});

	it("hands the keys back to the list before when the active one goes", () => {
		const heard: string[] = [];
		const root = listBuffer(() => heard.push("root"));
		const sub = listBuffer(() => heard.push("sub"));
		sub.release();
		runListCommand("list.down");
		root.release();
		expect(heard).toEqual(["root"]);
	});

	it("tells a list when it gains and loses the keys", () => {
		const seen: string[] = [];
		const root = listBuffer(() => {}, (on) => seen.push(`root:${on}`));
		const sub = listBuffer(() => {}, (on) => seen.push(`sub:${on}`));
		root.activate();
		root.activate();
		root.release();
		sub.release();
		expect(seen).toEqual(["root:true", "root:false", "sub:true", "sub:false", "root:true", "root:false", "sub:true", "sub:false"]);
	});
});
