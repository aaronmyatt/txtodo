<script lang="ts">
	// Help (task desktop-ui-revamp): the shortcuts come from the shared key table (`keyTable.ts`,
	// which reads keys.ts), the same one Settings > Shortcuts shows, so the two never disagree. The
	// prose below covers what a table cannot: search operators and what each screen does. Keep it
	// in step when a feature changes.
	import VersionInfo from "$lib/components/VersionInfo.svelte";
	import KeyTable from "$lib/components/KeyTable.svelte";
</script>

<main class="help-page">
	<div class="page">
		<h1>Help</h1>
		<p class="version"><VersionInfo /></p>

		<section aria-labelledby="h-editing">
			<h2 id="h-editing">Editing</h2>
			<p>
				The list is a plain text buffer you edit in place: click, type, and it saves as you go and
				again when you leave it. <code>id:</code> tags stay hidden but remain in the file. Lines past
				100 characters get a gentle wavy underline (Settings &rsaquo; Appearance turns it off).
			</p>
		</section>

		<section aria-labelledby="h-prompt">
			<h2 id="h-prompt">Prompt bar and Quick Add</h2>
			<p>
				The bar at the foot of every page adds a task to the open workspace's root list. While it has
				the keyboard, chips above it insert <code>(A)</code>&ndash;<code>(C)</code>, <code>x</code>,
				<code>+</code>, <code>@</code>, <code>due:</code>, <code>t:</code> and <code>rec:</code>. The
				mark beside it fills as tasks get done. The global hotkey focuses this bar when txtodo is in
				front, and opens a small Quick Add window anywhere else.
			</p>
		</section>

		<section aria-labelledby="h-search">
			<h2 id="h-search">Search</h2>
			<p>The header search filters the page you are on: lines on Tasks, rows on Universal, cards on Settings.</p>
			<dl class="ops">
				<dt><code>milk @shop</code></dt>
				<dd>Every word must appear (any case, anywhere in the line).</dd>
				<dt><code>-errand</code></dt>
				<dd>A leading dash leaves out lines with that word.</dd>
				<dt><code>is:open</code> / <code>is:done</code></dt>
				<dd>Only open or only done lines. On Universal, <code>is:done</code> shows done rows too.</dd>
			</dl>
		</section>

		<section aria-labelledby="h-keys">
			<h2 id="h-keys">Keyboard</h2>
			<KeyTable />
		</section>

		<section aria-labelledby="h-screens">
			<h2 id="h-screens">Screens</h2>
			<dl>
				<dt>Tasks</dt>
				<dd>
					The open workspace's root list. Double-click a line (or <kbd>⌘</kbd><kbd>↵</kbd>) to open its
					detail in a panel below: the line, its sub-list or notes, and "mark parent done" once every
					sub-task is.
				</dd>
				<dt>Universal</dt>
				<dd>
					Tasks from every workspace: counts, group by priority, due, project, context or workspace,
					filter by workspace and context, show done. The round mark completes a task where it lives,
					with Undo. Nested <code>ref:</code> sub-lists are not included.
				</dd>
				<dt>Settings</dt>
				<dd>
					Pin on top, theme and editor look, workspaces, shortcuts, device pairing, agent tokens and
					recent activity across workspaces.
				</dd>
				<dt>Workspace menu</dt>
				<dd>The name at the top left: switch, add, rename or remove workspaces.</dd>
				<dt>Conflicts</dt>
				<dd>
					A banner appears when two devices changed the same words. Review shows the difference and
					three choices: keep mine, keep theirs, or keep the merged text.
				</dd>
			</dl>
		</section>
	</div>
</main>

<style>
	.help-page {
		flex: 1;
		min-height: 0;
		overflow-y: auto;
		background: var(--color-base-100);
		color: var(--color-base-content);
	}

	.page {
		display: flex;
		flex-direction: column;
		gap: 1.75rem;
		max-width: 48rem;
		margin: 0 auto;
		padding: 1.5rem 1.25rem 3rem;
	}

	h1 {
		margin: 0;
		font-family: var(--font-mono);
		font-size: 1.5rem;
		letter-spacing: -0.05em;
	}

	.version {
		margin: -1rem 0 0;
		font-size: 0.75rem;
	}

	section {
		display: flex;
		flex-direction: column;
		gap: 0.6rem;
	}

	h2 {
		margin: 0;
		font-family: var(--font-mono);
		font-size: 1.125rem;
	}

	p,
	dd {
		margin: 0;
		font-size: 0.9375rem;
		line-height: 1.6;
	}

	dl {
		display: grid;
		grid-template-columns: max-content 1fr;
		gap: 0.5rem 1.25rem;
		margin: 0;
	}

	dt {
		font-family: var(--font-mono);
		font-size: 0.875rem;
		font-weight: 700;
	}

	code,
	kbd {
		font-family: var(--font-mono);
		font-size: 0.85em;
	}

	kbd {
		padding: 0 0.3rem;
		border: 2px solid color-mix(in srgb, var(--color-base-content) 35%, transparent);
		border-radius: 0.35rem;
	}

	@media (max-width: 40rem) {
		dl {
			grid-template-columns: 1fr;
		}
	}
</style>
