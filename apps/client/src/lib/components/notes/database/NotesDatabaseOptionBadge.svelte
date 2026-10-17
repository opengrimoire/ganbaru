<script lang="ts">
  import CollectionBadge from "$lib/components/collections/CollectionBadge.svelte";
  import { NOTES_TEXT_COLORS, notesBlockColorStyle } from "$lib/notes/blocks/color";
  let { label, color = "default" }: { label: string; color?: string } = $props();
  const resolvedColor = $derived(NOTES_TEXT_COLORS.find((entry) => entry === color) ?? "default");
</script>

<CollectionBadge {label} style={`${notesBlockColorStyle(resolvedColor)}; background-color: color-mix(in srgb, var(--notes-block-color) 15%, var(--card)); color: var(--foreground);`}>
  {#snippet leading()}<span class="size-2 shrink-0 rounded-full" style="background-color: var(--notes-block-color);" aria-hidden="true"></span>{/snippet}
</CollectionBadge>
