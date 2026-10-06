// Renders the menu-bar tray icon from assets/brand/tx-mark.svg (task desktop-ui-revamp): the mark
// in pure black on transparent, which macOS treats as a template image and recolours for light
// and dark menu bars (tray.rs sets icon_as_template). 16pt tall at @2x = 50 x 32 px.
// Writes src-tauri/icons/tray-template.rgba (raw RGBA, what tray.rs embeds: tauri::image::Image::new
// takes RGBA directly, so no PNG decoder crate is needed) and a .png beside it for people to look at.
// Run: node scripts/render-tray-icon.mjs
// Drawn by headless Chromium on a canvas, so no image tool is needed:
// https://developer.mozilla.org/en-US/docs/Web/API/CanvasRenderingContext2D/getImageData
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "@playwright/test";

const W = 50;
const H = 32;
const here = dirname(fileURLToPath(import.meta.url));
const svg = readFileSync(join(here, "../../../assets/brand/tx-mark.svg"), "utf8").replace(/currentColor/g, "#000");
const icons = join(here, "../src-tauri/icons");

const browser = await chromium.launch();
const page = await browser.newPage();
const { rgba, png } = await page.evaluate(
	async ({ svg, W, H }) => {
		const img = new Image();
		img.src = "data:image/svg+xml;charset=utf-8," + encodeURIComponent(svg);
		await img.decode();
		const canvas = document.createElement("canvas");
		canvas.width = W;
		canvas.height = H;
		const ctx = canvas.getContext("2d");
		ctx.drawImage(img, 0, 0, W, H);
		return { rgba: Array.from(ctx.getImageData(0, 0, W, H).data), png: canvas.toDataURL("image/png") };
	},
	{ svg, W, H }
);
await browser.close();
writeFileSync(join(icons, "tray-template.rgba"), Buffer.from(rgba));
writeFileSync(join(icons, "tray-template.png"), Buffer.from(png.split(",")[1], "base64"));
console.log(`wrote ${W}x${H} tray-template.rgba (${rgba.length} bytes) and .png`);
