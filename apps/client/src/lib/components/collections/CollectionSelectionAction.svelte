<script lang="ts">
  import type { CollectionIcon } from "./property-icons";
  import { cn } from "$lib/utils";

  /** One action button of a collection selection bar. */
  let { icon: Icon, label, destructive = false, disabled = false, onclick }: {
    icon: CollectionIcon;
    label: string;
    /** Marks an action that removes or archives the selected rows. */
    destructive?: boolean;
    disabled?: boolean;
    onclick: () => void;
  } = $props();
</script>

<button type="button"
  class={cn(
    "flex h-7 shrink-0 items-center gap-1 rounded-md border px-2 font-medium disabled:cursor-not-allowed disabled:opacity-50",
    destructive
      ? "border-destructive/30 bg-destructive/10 text-destructive hover:bg-destructive/15"
      : "border-border bg-card text-muted-foreground hover:bg-accent hover:text-foreground",
  )}
  {disabled} {onclick}>
  <Icon size={13} strokeWidth={1.75} aria-hidden="true" />
  <span>{label}</span>
</button>
