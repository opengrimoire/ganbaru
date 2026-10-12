<script lang="ts">
  import Check from "@lucide/svelte/icons/check";
  import { cn } from "$lib/utils";
  import { getCollectionSettingsNavigation } from "./collection-settings-context";
  import type { CollectionIcon } from "./property-icons";

  /** One action row in a collection column menu, property menu, or settings page, shared so Notes and Projects menus look and behave alike. */
  let { icon: Icon, label, detail, checked, destructive = false, disabled = false, onclick }: {
    /** Leading icon; choice lists inside submenus may omit it. */
    icon?: CollectionIcon;
    label: string;
    /** Muted trailing text, such as a choice's origin. */
    detail?: string;
    /** Shows a check and exposes the pressed state for toggles and current choices. */
    checked?: boolean;
    /** Marks an action that removes data. */
    destructive?: boolean;
    disabled?: boolean;
    onclick: () => void;
  } = $props();

  const settingsRow = getCollectionSettingsNavigation() !== undefined;
</script>

<button type="button" class={cn("menu-item min-w-0", destructive && "menu-item-destructive")}
  data-collection-settings-row={settingsRow ? "" : undefined} aria-pressed={checked} {disabled} {onclick}>
  {#if Icon}<Icon class={cn("size-3.5 shrink-0", !destructive && "text-muted-foreground")} strokeWidth={1.75} aria-hidden="true" />{/if}
  <span class="min-w-0 flex-1 truncate">{label}</span>
  {#if detail}<span class="shrink-0 text-muted-foreground">{detail}</span>{/if}
  {#if checked}<Check class="size-3.5 shrink-0" aria-hidden="true" />{/if}
</button>
