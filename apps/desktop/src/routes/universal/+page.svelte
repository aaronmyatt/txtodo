<script lang="ts">
	// Universal view route (ADR 0025, task desktop-universal-view): thin mount, same pattern as
	// `/devices` — the real screen is self-contained in `UniversalView.svelte`.
	import UniversalView from "$lib/components/UniversalView.svelte";
</script>

<UniversalView />
