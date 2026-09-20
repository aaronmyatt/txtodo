// Vitest: https://vitest.dev/api/
import { describe, expect, it } from "vitest";
import { daemonMismatch, versionLabel, type BuildInfo } from "../versionInfo";

const app = { version: "0.0.2", release_date: "2026-09-20" };
const info = (daemon_version: string, daemon_release_date: string): BuildInfo => ({
	...app,
	daemon_version,
	daemon_release_date
});

describe("versionLabel", () => {
	it("is the short UI form", () => {
		expect(versionLabel("0.0.2", "2026-09-20")).toBe("v0.0.2 · 2026-09-20");
	});
});

describe("daemonMismatch", () => {
	it("says nothing when the daemon is this app's build", () => {
		expect(daemonMismatch(info("0.0.2", "2026-09-20"))).toBeNull();
	});

	it("says nothing when there is no daemon to compare with", () => {
		expect(daemonMismatch(info("", ""))).toBeNull();
	});

	it("names both builds when the version or only the date differs", () => {
		const older = daemonMismatch(info("0.0.1", "2026-09-17"));
		expect(older).toContain("v0.0.1 · 2026-09-17");
		expect(older).toContain("v0.0.2 · 2026-09-20");
		expect(daemonMismatch(info("0.0.2", "2026-09-19"))).toContain("2026-09-19");
	});

	it("treats a daemon that sends no release date as an older build", () => {
		const warning = daemonMismatch(info("0.0.2", ""));
		expect(warning).toContain("sent no release date");
		expect(warning).toContain("txtodo daemon install");
	});
});
