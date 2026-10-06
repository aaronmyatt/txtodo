// The detail view hides its parent line's `ref:` tag (root todo line "ref tag path is visible in the
// desktop detail view": deleting it there cut the link and minted a new folder). The tag is
// plumbing the panel already stands for, so an edit there never changes it: the saved line carries
// the original `ref:` and no other. The TUI does the same (`state_tasks::with_ref`).
import { findRefTag } from "./lineInfo";

/** `edited` as it is when its one `ref:` is still `original`'s (or `original` had none); else with
 * every `ref:` word dropped and `original`'s put back at the end. */
export function keepRefTag(original: string, edited: string): string {
	const ref = findRefTag(original);
	if (!ref) return edited;
	const refs = edited.split(" ").filter((w) => w.startsWith("ref:"));
	if (refs.length === 1 && refs[0] === `ref:${ref.slug}`) return edited; // intact, where it was
	const words = edited.split(" ").filter((w) => !w.startsWith("ref:"));
	while (words.length > 0 && words[words.length - 1] === "") words.pop();
	return [...words, `ref:${ref.slug}`].join(" ");
}
