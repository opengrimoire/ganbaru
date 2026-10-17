<script lang="ts" generics="Value extends string">
  import CollectionMenu from "./CollectionMenu.svelte";
  import CollectionMenuItem from "./CollectionMenuItem.svelte";
  import type { CollectionIcon } from "./property-icons";

  /** One choice in a collection menu select. */
  interface CollectionMenuSelectOption {
    value: Value;
    label: string;
    icon?: CollectionIcon;
  }

  /**
   * A menu row that shows the current choice and opens a checked list of choices.
   * Inside collection settings it drills into a page; inside another menu it opens a submenu.
   */
  let { label, icon, value, options, disabled = false, onChange }: {
    label: string;
    icon?: CollectionIcon;
    value: Value;
    options: readonly CollectionMenuSelectOption[];
    disabled?: boolean;
    onChange: (value: Value) => void;
  } = $props();

  const selected = $derived(options.find((option) => option.value === value));
</script>

<CollectionMenu {label} {icon} kind="properties" fullWidth {disabled} summary={selected?.label ?? ""}>
  {#each options as option (option.value)}
    <CollectionMenuItem icon={option.icon} label={option.label} checked={option.value === value} {disabled}
      onclick={() => { if (option.value !== value) onChange(option.value); }} />
  {/each}
</CollectionMenu>
