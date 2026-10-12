<script lang="ts">
  import ChevronRight from "@lucide/svelte/icons/chevron-right";

  /**
   * The compact action that sits right after a heading, as Settings > Chat > Teammates does for channel access. Its
   * height never exceeds the heading line, so sections with and without an action share the same baseline.
   */
  let {
    label,
    element = $bindable(null),
    expanded = false,
    popup = "dialog",
    onclick,
  }: {
    label: string;
    element?: HTMLButtonElement | null;
    expanded?: boolean;
    popup?: "dialog" | "menu";
    onclick: () => void;
  } = $props();
</script>

<button bind:this={element} type="button" class="contacts-heading-action" aria-haspopup={popup} aria-expanded={expanded} {onclick}>
  <span>{label}</span>
  <ChevronRight size={13} />
</button>

<style>
  .contacts-heading-action { display: inline-flex; min-height: 1.25rem; flex: 0 0 auto; align-items: center; justify-content: center; gap: 0.22rem; padding: 0.05rem 0.1rem; color: var(--muted-foreground); font-size: calc(0.666667rem * var(--type-scale)); font-weight: 600; white-space: nowrap; }
  .contacts-heading-action:hover, .contacts-heading-action[aria-expanded="true"] { color: var(--foreground); }
</style>
