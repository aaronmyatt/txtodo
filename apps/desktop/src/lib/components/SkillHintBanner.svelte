<script lang="ts">
	// Onboarding nudge (root todo.txt agent-skill-install, tasks/agent-skill-install/todo.txt
	// id:01M2HTCV56RRB7ETP8GG17PVN1): mirrors txtodo-cli's `doctor` skill row and txtodo-tui's
	// status-line hint in this Tauri/Svelte stack. Advisory only — dismissing it never installs
	// anything and never re-checks until the next app launch, same "session-only" contract as
	// ConflictBanner's dismiss.
	import { onMount } from "svelte";
	import { skillHintNeeded } from "$lib/daemon";

	let needed = $state(false);
	let dismissed = $state(false);

	onMount(async () => {
		needed = await skillHintNeeded();
	});

	function dismiss() {
		dismissed = true;
	}
</script>

{#if needed && !dismissed}
	<div class="banner tx-strip" role="status">
		<span>No agent playbook installed — run <code>txtodo skill install</code> in a terminal</span>
		<button type="button" class="dismiss" onclick={dismiss} aria-label="Dismiss">×</button>
	</div>
{/if}

