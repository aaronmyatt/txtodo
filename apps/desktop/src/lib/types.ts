// Shared shapes for the detail view (tasks/desktop-detail-view/notes.md, plan §3.2). Kept in their
// own module (rather than folded into $lib/daemon.ts) because these describe *navigation state the
// desktop app invents*, not a daemon wire type — there's no `DetailParamsDto`/`BreadcrumbStepDto`
// on the Rust side to mirror.

/**
 * One level of the detail-view navigation stack: the workspace-relative file holding the pinned
 * parent line, and that line's 1-based number within it. The root (no detail view open) is an
 * empty stack, not a `DetailParams` with a sentinel — see `MainView.svelte`.
 */
export interface DetailParams {
	file: string;
	line: number;
	/** Set only when this detail level was opened from the universal view (ADR 0025, task
	 * desktop-universal-view) rather than by navigating within the currently open workspace: the
	 * absolute root of the workspace this level belongs to, so `Breadcrumb` can show which
	 * project owns it. Undefined for ordinary same-workspace navigation. */
	workspaceRoot?: string;
}

/** One rendered breadcrumb segment; today this is exactly a `DetailParams`, but it's named and
 * exported separately from the navigation stack's own type so `Breadcrumb.svelte`'s prop doesn't
 * read as "here's the app's navigation state" to a component that only ever renders it. */
export type BreadcrumbStep = DetailParams;
