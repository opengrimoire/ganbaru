<script lang="ts">
  import RotateCcw from "@lucide/svelte/icons/rotate-ccw";
  import { cn } from "$lib/utils";
  import { getLocalization } from "$lib/i18n/translator.svelte";

  /** Square button beside a settings control that returns it to its default value. */
  let {
    canReset,
    onReset,
    label,
    class: className,
  }: {
    /** Whether the control differs from its default; the button stays visible but inert otherwise. */
    canReset: boolean;
    onReset: () => void;
    /** Accessible name; defaults to the generic reset label. */
    label?: string;
    class?: string;
  } = $props();

  const { t } = getLocalization();
</script>

<button
  type="button"
  onclick={onReset}
  disabled={!canReset}
  aria-label={label ?? t("common.reset")}
  data-app-tooltip-disabled="true"
  class={cn(
    "flex size-(--control-height) shrink-0 items-center justify-center rounded-md border border-border bg-secondary text-secondary-foreground transition-colors",
    canReset ? "hover:bg-accent hover:text-accent-foreground" : "cursor-not-allowed opacity-40",
    className,
  )}
>
  <RotateCcw size={12} strokeWidth={2.25} />
</button>
