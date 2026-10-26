// Images in the notes editor (task enable-dragging-and-dropping-images-to-a): a line that links an
// `assets/…` image as markdown (`![name](assets/…)`) gets the image drawn under it, and a dropped
// or pasted image file is saved beside notes.md and linked at the drop point. The bytes come from
// the Rust side (`commands_assets.rs`), shown through a blob URL, so no file:// access is needed.
// CM6 block widgets come from a state field, not a view plugin:
// https://codemirror.net/docs/ref/#view.Decoration^widget
// Blob URLs: https://developer.mozilla.org/en-US/docs/Web/API/URL/createObjectURL_static
import { RangeSetBuilder, StateField, type EditorState, type Extension } from "@codemirror/state";
import { Decoration, EditorView, WidgetType, type DecorationSet } from "@codemirror/view";

/** `![alt](assets/file.png)`: the alt text and the link. */
const IMAGE_LINK = /!\[([^\]]*)\]\((assets\/[^)\s]+)\)/;

/** The `assets/…` link and alt text on `line`, if it links an image. */
export function imageLink(line: string): { alt: string; link: string } | null {
	const m = IMAGE_LINK.exec(line);
	return m ? { alt: m[1], link: m[2] } : null;
}

/** What a dropped file becomes in the text: one markdown image line. */
export function imageMarkdown(name: string, link: string): string {
	const alt = name.replace(/\.[^.]+$/, "").replace(/[[\]]/g, "");
	return `![${alt}](${link})`;
}

const MIME: Record<string, string> = { png: "image/png", jpg: "image/jpeg", jpeg: "image/jpeg", gif: "image/gif", webp: "image/webp" };

export interface ImageHost {
	/** The notes.md path the links are relative to; "" before the notes exist. */
	notesPath: () => string;
	/** The bytes of a link (`readNoteAsset`). */
	read: (notesPath: string, link: string) => Promise<number[]>;
}

class ImageWidget extends WidgetType {
	constructor(
		readonly link: string,
		readonly alt: string,
		readonly host: ImageHost,
		readonly cache: Map<string, Promise<string>>
	) {
		super();
	}

	eq(other: ImageWidget): boolean {
		return other.link === this.link && other.alt === this.alt;
	}

	toDOM(): HTMLElement {
		const img = document.createElement("img");
		img.className = "cm-note-image";
		img.alt = this.alt;
		const notes = this.host.notesPath();
		const key = `${notes}\n${this.link}`;
		let url = this.cache.get(key);
		if (!url) {
			const ext = this.link.split(".").pop()?.toLowerCase() ?? "";
			url = this.host
				.read(notes, this.link)
				.then((bytes) => URL.createObjectURL(new Blob([new Uint8Array(bytes)], { type: MIME[ext] ?? "" })));
			this.cache.set(key, url);
		}
		url.then((src) => (img.src = src)).catch(() => (img.alt = `${this.alt} (not on this device)`));
		return img;
	}
}

/** Draws each linked image under its line. */
export function noteImages(host: ImageHost): Extension {
	const cache = new Map<string, Promise<string>>();
	const build = (state: EditorState): DecorationSet => {
		const b = new RangeSetBuilder<Decoration>();
		for (let i = 1; i <= state.doc.lines; i++) {
			const line = state.doc.line(i);
			const hit = imageLink(line.text);
			if (hit) {
				b.add(line.to, line.to, Decoration.widget({ widget: new ImageWidget(hit.link, hit.alt, host, cache), block: true, side: 1 }));
			}
		}
		return b.finish();
	};
	const field = StateField.define<DecorationSet>({
		create: build,
		update: (deco, tr) => (tr.docChanged ? build(tr.state) : deco),
		provide: (f) => EditorView.decorations.from(f)
	});
	return [
		field,
		EditorView.baseTheme({ ".cm-note-image": { display: "block", maxWidth: "100%", maxHeight: "16rem", margin: "0.25rem 0" } })
	];
}

/** The image files among `files`. */
export function imageFiles(files: FileList | null | undefined): File[] {
	return Array.from(files ?? []).filter((f) => f.type.startsWith("image/") && f.type !== "image/svg+xml");
}
