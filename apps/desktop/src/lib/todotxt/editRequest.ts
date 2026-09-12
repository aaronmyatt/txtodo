// Shared shape for "a line's hover-pencil was clicked" — raised by `FileView` (optionally, via
// its `onEditRequest` prop) and consumed by whichever component hosts the edit popover
// (`MainView` at the top level; `FileView` falls back to hosting its own when no callback is
// wired, e.g. a future detail view's nested instances).
import type { TaskRef } from "$lib/daemon";

export interface EditRequest {
	/** Workspace-relative path of the file the line belongs to, so a shared host knows where to `apply` the edit. */
	path: string;
	initialLine: string;
	taskRef: TaskRef;
	anchor: HTMLElement;
}
