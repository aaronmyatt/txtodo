// The monoline icon set from the c2 mockup (docs/design/desktop-ui-revamp/shared/txtodo-ui.js):
// 24-unit grid, drawn by `Icon.svelte` with a 2.75 stroke and round caps/joins, like the mark.
// Each value is the inner SVG markup of one icon; a constant, never user input.

export const ICONS = {
	search: '<circle cx="11" cy="11" r="6.5"/><path d="M16 16l4.5 4.5"/>',
	plus: '<path d="M12 5v14M5 12h14"/>',
	sun: '<circle cx="12" cy="12" r="4"/><path d="M12 3v2M12 19v2M3 12h2M19 12h2M5.6 5.6l1.4 1.4M17 17l1.4 1.4M5.6 18.4L7 17M17 7l1.4-1.4"/>',
	moon: '<path d="M20 14.5A8 8 0 1 1 9.5 4a6.5 6.5 0 0 0 10.5 10.5z"/>',
	pin: '<path d="M9 3h6l-1 6 3 3H7l3-3-1-6zM12 12v9"/>',
	devices: '<rect x="3" y="5" width="12" height="9" rx="2"/><path d="M7 18h4M9 14v4"/><rect x="17" y="8" width="4" height="9" rx="1.5"/>',
	stack: '<path d="M12 4l8 4-8 4-8-4 8-4z"/><path d="M4 12l8 4 8-4M4 16l8 4 8-4"/>',
	help: '<circle cx="12" cy="12" r="9"/><path d="M9.5 9.5a2.5 2.5 0 1 1 3.5 2.3c-.7.4-1 1-1 1.7M12 17h.01"/>',
	left: '<path d="M15 6l-6 6 6 6"/>',
	right: '<path d="M9 6l6 6-6 6"/>',
	down: '<path d="M6 9l6 6 6-6"/>',
	back: '<path d="M19 12H5M11 6l-6 6 6 6"/>',
	folder: '<path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V7z"/>',
	file: '<path d="M7 3h7l5 5v11a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2z"/><path d="M14 3v5h5"/>',
	note: '<path d="M5 4h14v12l-4 4H5V4z"/><path d="M9 9h6M9 13h3"/>',
	check: '<path d="M5 12l4.5 4.5L19 7"/>',
	hook: '<path d="M4 9h10l5 6"/>',
	x: '<path d="M6 6l12 12M18 6L6 18"/>',
	sync: '<path d="M20 11a8 8 0 0 0-14-4L4 9M4 4v5h5M4 13a8 8 0 0 0 14 4l2-2M20 20v-5h-5"/>',
	warn: '<path d="M12 4l9 16H3L12 4zM12 10v4M12 17.5h.01"/>',
	enter: '<path d="M20 6v6a2 2 0 0 1-2 2H5M9 10l-4 4 4 4"/>',
	dots: '<path d="M5 12h.01M12 12h.01M19 12h.01"/>',
	list: '<path d="M4 7h16M4 12h16M4 17h10"/>',
	bolt: '<path d="M13 3L5 13h6l-1 8 8-10h-6l1-8z"/>',
	key: '<circle cx="8" cy="15" r="4"/><path d="M11 12l9-9M16 7l3 3"/>',
	laptop: '<rect x="5" y="5" width="14" height="10" rx="2"/><path d="M3 19h18"/>',
	phone: '<rect x="8" y="3" width="8" height="18" rx="2.5"/><path d="M11 17.5h2"/>',
	sliders: '<path d="M4 7h9M19 7h1M4 17h1M11 17h9"/><circle cx="16" cy="7" r="2.5"/><circle cx="8" cy="17" r="2.5"/>',
	copy: '<rect x="8" y="8" width="12" height="12" rx="2.5"/><path d="M16 8V6.5A2.5 2.5 0 0 0 13.5 4H6.5A2.5 2.5 0 0 0 4 6.5v7A2.5 2.5 0 0 0 6.5 16H8"/>',
	trash: '<path d="M5 7h14M10 7V5h4v2M7 7l1 12h8l1-12"/>',
} as const;

/** The name of an icon in the set. */
export type IconName = keyof typeof ICONS;
