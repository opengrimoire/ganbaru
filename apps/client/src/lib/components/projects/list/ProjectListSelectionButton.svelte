<script lang="ts">
  import CollectionSelectionCheckbox from "$lib/components/collections/CollectionSelectionCheckbox.svelte";

  type ProjectListSelectionButtonMode = "section" | "group";

  let {
    mode,
    allSelected,
    partiallySelected,
    disabled,
    ariaLabel,
    onToggle,
  }: {
    mode: ProjectListSelectionButtonMode;
    allSelected: boolean;
    partiallySelected: boolean;
    disabled: boolean;
    ariaLabel: string;
    onToggle: () => void;
  } = $props();
</script>

<CollectionSelectionCheckbox checked={allSelected} indeterminate={partiallySelected} label={ariaLabel} {disabled} {onToggle}
  revealClass={mode === "section"
    ? "[@media(hover:hover)]:group-hover/section-header:opacity-100 [@media(hover:hover)]:group-has-focus-visible/section-header:opacity-100"
    : "[@media(hover:hover)]:group-hover/list-group-header:opacity-100 [@media(hover:hover)]:group-has-focus-visible/list-group-header:opacity-100"} />
