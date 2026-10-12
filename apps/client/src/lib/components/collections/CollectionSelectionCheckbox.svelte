<script lang="ts">
  import Checkbox from "$lib/components/ui/Checkbox.svelte";
  import { cn } from "$lib/utils";

  /**
   * The leading selection control of a collection row or group header. It stays hidden until its row is hovered or
   * focused, and shows whenever something is selected, so a selection is never invisible.
   */
  let { checked, indeterminate = false, label, disabled = false, revealClass = "[@media(hover:hover)]:group-hover/row:opacity-100 [@media(hover:hover)]:group-focus-within/row:opacity-100", onToggle }: {
    checked: boolean;
    /** Shows that only part of a group is selected. */
    indeterminate?: boolean;
    label: string;
    disabled?: boolean;
    /**
     * Hover and focus classes of the group that reveals the control, under the same `[@media(hover:hover)]` variant
     * that hides it; Tailwind needs them as literals.
     */
    revealClass?: string;
    onToggle: () => void;
  } = $props();
</script>

<span class={cn("flex h-7 items-center justify-center", !(checked || indeterminate) && cn("[@media(hover:hover)]:opacity-0", revealClass))}>
  <Checkbox {checked} {indeterminate} {label} {disabled} onChange={() => onToggle()} />
</span>
