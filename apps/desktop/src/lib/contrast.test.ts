// AA contrast for the theme colours (task desktop-ui-revamp, accessibility pass), read straight
// from app.css so a palette change cannot slip under 4.5:1 for normal text. WCAG 2.2 relative
// luminance and contrast ratio: https://www.w3.org/TR/WCAG22/#dfn-contrast-ratio
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

const CSS = readFileSync(join(__dirname, "..", "app.css"), "utf8");

/** The `--name: #hex;` pairs in the first block that starts with `selector {`. */
function vars(selector: string): Record<string, string> {
	const start = CSS.indexOf(`${selector} {`);
	const body = CSS.slice(start, CSS.indexOf("\n}", start));
	return Object.fromEntries([...body.matchAll(/(--[a-z0-9-]+):\s*(#[0-9a-f]{6});/gi)].map((m) => [m[1], m[2]]));
}

function luminance(hex: string): number {
	const [r, g, b] = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255);
	const lin = (c: number) => (c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4);
	return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
}

function ratio(a: string, b: string): number {
	const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
	return (hi + 0.05) / (lo + 0.05);
}

/** `fg` at `alpha` over `bg`, as color-mix(in srgb, fg alpha%, transparent) paints on bg. */
function blend(fg: string, bg: string, alpha: number): string {
	const ch = (h: string, i: number) => parseInt(h.slice(i, i + 2), 16);
	return "#" + [1, 3, 5].map((i) => Math.round(ch(fg, i) * alpha + ch(bg, i) * (1 - alpha)).toString(16).padStart(2, "0")).join("");
}

const THEMES = { light: vars(":root"), dark: vars('[data-theme="dark"]') };
// Text the reader must read. --tok-id-tag is left out on purpose: id: tags are hidden, and where
// shown they are meant to recede.
const TOKENS = ["--tok-completion-marker", "--tok-priority", "--tok-date", "--tok-project", "--tok-context", "--tok-tag-key", "--tok-tag-value"];

describe.each(Object.entries(THEMES))("%s theme meets AA (4.5:1)", (_, v) => {
	const bg = v["--color-base-100"];
	const ink = v["--color-base-content"];

	it("ink on paper, and the inverse", () => {
		expect(ratio(ink, bg)).toBeGreaterThanOrEqual(4.5);
		expect(ratio(v["--color-primary-content"], v["--color-primary"])).toBeGreaterThanOrEqual(4.5);
	});

	it.each(TOKENS)("%s on paper", (tok) => {
		expect(ratio(v[tok], bg)).toBeGreaterThanOrEqual(4.5);
	});

	it("muted text (60% ink, the faintest used for words) on paper", () => {
		expect(ratio(blend(ink, bg, 0.6), bg)).toBeGreaterThanOrEqual(4.5);
	});

	it.each(["success", "warning", "error"])("%s badges", (name) => {
		expect(ratio(v[`--color-${name}-content`], v[`--color-${name}`])).toBeGreaterThanOrEqual(4.5);
	});
});
