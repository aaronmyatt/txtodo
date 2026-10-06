// Due badges (task desktop-ui-revamp, Universal): a `due:` date read as a local calendar day
// against `localToday()` (ADR 0011: the user's own date, never UTC "now").

const DATE_RE = /^\d{4}-\d{2}-\d{2}$/;
const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

export type DueTone = "overdue" | "today" | "soon" | "later";

export interface DueLabel {
	text: string;
	tone: DueTone;
	/** Days from today; negative when overdue. */
	days: number;
}

/** Whole days from `from` to `to` (both `YYYY-MM-DD`). Date.UTC keeps it free of DST shifts:
 * https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date/UTC */
export function daysBetween(from: string, to: string): number {
	const utc = (d: string) => Date.UTC(+d.slice(0, 4), +d.slice(5, 7) - 1, +d.slice(8, 10));
	return Math.round((utc(to) - utc(from)) / 86_400_000);
}

/** `overdue 2d` / `today` / `tomorrow` / `in 4d` (up to 7) / `Oct 15`; null for no or a bad date. */
export function dueLabel(due: string | null | undefined, today: string): DueLabel | null {
	if (!due || !DATE_RE.test(due)) return null;
	const days = daysBetween(today, due);
	if (days < 0) return { text: `overdue ${-days}d`, tone: "overdue", days };
	if (days === 0) return { text: "today", tone: "today", days };
	if (days === 1) return { text: "tomorrow", tone: "soon", days };
	if (days <= 7) return { text: `in ${days}d`, tone: "soon", days };
	return { text: `${MONTHS[+due.slice(5, 7) - 1]} ${+due.slice(8, 10)}`, tone: "later", days };
}
