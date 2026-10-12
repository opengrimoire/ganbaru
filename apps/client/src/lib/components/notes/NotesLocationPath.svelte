<script lang="ts">
  import { compactSourceLocation } from "$lib/notes/database/source-location";

  /**
   * A muted one-line route such as "Personal / … / Tasks" under a row title. Screen readers get the full path; the
   * visible copy collapses its middle and truncates the outer segments before the innermost one.
   */
  let { segments }: { segments: readonly string[] } = $props();

  const fullPath = $derived(segments.join(" / "));
</script>

{#if segments.length > 0}
  <span class="sr-only">{fullPath}</span>
  <span class="flex min-w-0 text-panel-detail text-muted-foreground" aria-hidden="true">
    {#each compactSourceLocation(segments) as segment, index (index)}
      {#if index > 0}<span class="shrink-0 px-1">/</span>{/if}
      {#if segment.kind === "gap"}
        <span class="shrink-0">…</span>
      {:else}
        <span class={["min-w-8 truncate", segment.last ? "shrink" : "shrink-4"]}>{segment.text}</span>
      {/if}
    {/each}
  </span>
{/if}
