// The Settings page's sections (task desktop-ui-revamp): ids for deep links (`/settings#tokens`),
// nav labels, and the words the header search matches a card by.

export interface Section {
	id: string;
	title: string;
	/** Extra words a search finds this card by, beyond its title. */
	keywords: string;
}

export const SECTIONS: Section[] = [
	{ id: "general", title: "General", keywords: "pin on top window menu bar tray daemon restart retry save" },
	{ id: "appearance", title: "Appearance", keywords: "theme light dark system font text size line height numbers hint preview" },
	{ id: "workspaces", title: "Workspaces", keywords: "folder project add remove open switch default" },
	{ id: "shortcuts", title: "Shortcuts", keywords: "keys keyboard hotkey quick add chord" },
	{ id: "devices", title: "Devices", keywords: "pair pairing qr phone laptop sync sas" },
	{ id: "tokens", title: "Tokens", keywords: "agent api capability scope secret revoke mcp" },
	{ id: "activity", title: "Activity", keywords: "log history ops feed recent" }
];

/** Whether every search term appears in the card's title or keywords (case-insensitive). */
export function sectionMatches(section: Section, query: string): boolean {
	const hay = `${section.title} ${section.keywords}`.toLowerCase();
	return query
		.toLowerCase()
		.split(/\s+/)
		.filter(Boolean)
		.every((term) => hay.includes(term));
}

/** The section the nav highlights: the first visible card whose top is at or above `line` (px
 * from the scroller's top), else the first visible one; the last visible one once the scroller is
 * at its end, since a short last card can never reach the top. `tops` are in card order. */
export function activeSection(
	tops: { id: string; top: number }[],
	line: number,
	atEnd: boolean
): string | null {
	if (tops.length === 0) return null;
	if (atEnd) return tops[tops.length - 1].id;
	let current = tops[0].id;
	for (const t of tops) if (t.top <= line) current = t.id;
	return current;
}
