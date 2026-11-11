<script lang="ts">
  import { BUILD_PLATFORM_PROFILE } from "$lib/platform";
  import { cn } from "$lib/utils";

  /** `compact` fits a floating panel row; `default` fits Settings rows. */
  type SwitchSize = "default" | "compact";

  /** Track, thumb, and checked travel classes for one switch size. */
  interface SwitchDimensions {
    track: string;
    thumb: string;
    travel: string;
  }

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

  /* The mobile shell enlarges the default switch so it sits level with the touch-height fields and buttons around it; the hit area extends to the 44px row target. */
  const touchShell = BUILD_PLATFORM_PROFILE.shell === "mobile";

  const dimensions = $derived<SwitchDimensions>(
    size === "compact"
      ? { track: "h-5 w-8", thumb: "size-3.5", travel: "translate-x-3" }
      : touchShell
        ? { track: "relative h-7 w-12 after:absolute after:-inset-2", thumb: "size-5.5", travel: "translate-x-5" }
        : { track: "h-6 w-10", thumb: "size-4.5", travel: "translate-x-4" },
  );
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
    dimensions.track,
    checked
      ? "border-primary bg-primary"
      : "border-border bg-foreground/10",
  )}
>
  <!-- The thumb travels the track's inner width: track width minus border, padding, and thumb. -->
  <span
    class={cn(
      "block rounded-full bg-background transition-transform",
      dimensions.thumb,
      checked ? dimensions.travel : "translate-x-0",
    )}
  ></span>
</button>
