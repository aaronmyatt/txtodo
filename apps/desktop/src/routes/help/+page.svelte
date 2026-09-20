<script lang="ts">
	// Help route (root todo.txt: "add a '?' top nav button that links to a help page with
	// functionality and keybinding descriptions"). Static content only — everything below is a
	// direct transcription of this app's own shortcuts/features, not a generic template; keep it
	// in sync by hand when a keymap or feature changes (no generator exists for this yet).
	import ThemeToggle from "$lib/components/ThemeToggle.svelte";
	import VersionInfo from "$lib/components/VersionInfo.svelte";
</script>

<main class="help-page">
	<div class="top-nav">
		<a href="/">‹ Back</a>
		<ThemeToggle />
	</div>
	<h1>Help</h1>
	<p><VersionInfo /></p>

	<section>
		<h2>Editing</h2>
		<p>
			The task list is always a live, directly-editable buffer — click to place a cursor, type,
			and it saves on blur or Cmd/Ctrl+S. There is no separate "raw mode" and no popover for the
			main list or a task's detail view; what you see is what gets written. <code>id:</code> tags
			are always hidden (still present in the file underneath).
		</p>
	</section>

	<section>
		<h2>Keyboard shortcuts</h2>
		<table>
			<thead>
				<tr>
					<th>Shortcut</th>
					<th>Where</th>
					<th>Does</th>
				</tr>
			</thead>
			<tbody>
				<tr>
					<td><kbd>Cmd/Ctrl</kbd>+<kbd>Enter</kbd></td>
					<td>Task list</td>
					<td>Opens the detail view for the line under the cursor (same as double-click)</td>
				</tr>
				<tr>
					<td><kbd>Cmd/Ctrl</kbd>+<kbd>S</kbd></td>
					<td>Any editor</td>
					<td>Commits the current edit immediately (otherwise it commits on blur)</td>
				</tr>
				<tr>
					<td><kbd>Esc</kbd></td>
					<td>Any editor, while editing</td>
					<td>Discards the edit, reverting to the last saved text</td>
				</tr>
				<tr>
					<td><kbd>Esc</kbd></td>
					<td>Workspace sidebar, conflict review</td>
					<td>Closes the panel</td>
				</tr>
				<tr>
					<td><kbd>Cmd/Ctrl</kbd>+<kbd>Z</kbd> / <kbd>Shift</kbd>+<kbd>Cmd/Ctrl</kbd>+<kbd>Z</kbd></td>
					<td>Detail view's title line, notes, Quick Add</td>
					<td
						>Undo / redo — <strong>not available</strong> in the main task list itself, only in these
						single-purpose editors</td
					>
				</tr>
				<tr>
					<td><kbd>Cmd</kbd>+<kbd>Shift</kbd>+<kbd>Space</kbd></td>
					<td>Anywhere (global)</td>
					<td>
						Opens Quick Add to append a task to the root list — same shortcut on Windows uses the
						Windows key, on Linux the Super key, in place of Cmd
					</td>
				</tr>
			</tbody>
		</table>
	</section>

	<section>
		<h2>Features</h2>
		<dl>
			<dt>Universal view</dt>
			<dd>
				Every open task across every registered workspace in one list, grouped by priority and
				filterable by <code>@context</code>. Clicking a task switches to its workspace and jumps to
				the line. Nested <code>ref:</code> sub-lists are not included.
			</dd>
			<dt>Workspace switcher</dt>
			<dd>
				The ▤ button, top left. Switch, add or remove registered workspaces; a workspace whose
				folder no longer exists shows greyed out and labeled "missing" rather than disappearing.
				Its Activity tab shows the newest ops across every workspace.
			</dd>
			<dt>Devices &amp; agents</dt>
			<dd>
				Pair a new device by QR code (with a two-device confirm step, never automatic), manage
				scoped API tokens, and read this workspace's own activity feed.
			</dd>
			<dt>Quick Add</dt>
			<dd>
				A hidden popover window, opened by the global shortcut above or the tray/menu-bar icon.
				Always appends to the root list.
			</dd>
			<dt>Detail view</dt>
			<dd>
				Double-click (or Cmd/Ctrl+Enter) a line to open it: the line itself stays editable at the
				top, below it is either that task's sub-list or its notes, and a "Mark parent done" button
				appears once every sub-task is complete.
			</dd>
			<dt>Conflicts</dt>
			<dd>
				A banner appears when a task needs review after a concurrent edit. Review shows a
				character-level diff and three resolutions — keep mine, keep theirs, or keep the merged
				text — dismissing the banner never clears the flag on its own.
			</dd>
			<dt>Pin on top</dt>
			<dd>The 📌 button keeps the window above others; remembered between launches.</dd>
		</dl>
	</section>
</main>

<style>
	.help-page {
		padding: 2rem;
		max-width: 48rem;
		margin: 0 auto;
		display: flex;
		flex-direction: column;
		gap: 1.5rem;
		background: var(--color-bg);
		color: var(--color-text);
	}

	.top-nav {
		display: flex;
		align-items: center;
		justify-content: space-between;
	}

	h1 {
		margin: 0;
	}

	section {
		display: flex;
		flex-direction: column;
		gap: 0.5rem;
	}

	h2 {
		font-size: 1.05rem;
		margin: 0;
	}

	p {
		margin: 0;
		line-height: 1.5;
	}

	table {
		border-collapse: collapse;
		width: 100%;
	}

	th,
	td {
		text-align: left;
		padding: 0.4rem 0.6rem;
		border-bottom: 1px solid var(--color-border-subtle);
		vertical-align: top;
	}

	kbd {
		font-family: var(--font-mono);
		background: var(--color-hover-overlay);
		border-radius: 4px;
		padding: 0.1rem 0.35rem;
		font-size: 0.85em;
	}

	dl {
		margin: 0;
		display: flex;
		flex-direction: column;
		gap: 0.75rem;
	}

	dt {
		font-weight: 600;
	}

	dd {
		margin: 0.15rem 0 0;
		line-height: 1.5;
		color: var(--color-text-muted);
	}
</style>
