<script lang="ts">
  import { cn } from "$lib/utils";

  /** `compact` fits a floating panel row; `default` fits Settings rows. */
  type SwitchSize = "default" | "compact";

  let {
    checked,
    onChange,
    ariaLabel,
    disabled = false,
    size = "default",
  }: {
    checked: boolean;
    onChange: (checked: boolean) => void;
    ariaLabel: string;
    disabled?: boolean;
    size?: SwitchSize;
  } = $props();
</script>

<!-- The off track tints its host surface, so it reads the same in Settings and on lighter popovers. -->
<button
  type="button"
  role="switch"
  aria-checked={checked}
  aria-label={ariaLabel}
  data-app-tooltip-disabled="true"
  {disabled}
  onclick={() => {
    if (!disabled) onChange(!checked);
  }}
  class={cn(
    "inline-flex shrink-0 items-center rounded-full border p-0.5 transition-colors disabled:cursor-not-allowed",
    size === "compact" ? "h-5 w-8" : "h-6 w-10",
    checked
      ? "border-primary bg-primary"
      : "border-border bg-foreground/10",
  )}
>
  <!-- The thumb travels the track's inner width: track width minus border, padding, and thumb. -->
  <span
    class={cn(
      "block rounded-full bg-background transition-transform",
      size === "compact" ? "size-3.5" : "size-4.5",
      checked ? (size === "compact" ? "translate-x-3" : "translate-x-4") : "translate-x-0",
    )}
  ></span>
</button>
