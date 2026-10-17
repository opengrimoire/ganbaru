<script lang="ts">
  let {
    mode,
    strokeWidth = 2,
    class: className = "",
  }: {
    mode: "side" | "center" | "full";
    strokeWidth?: number;
    class?: string;
  } = $props();
</script>

<svg
  class={className}
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width={strokeWidth}
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"
  focusable="false"
>
  <rect x="3.5" y="5" width="17" height="14" rx="2.6" />
  {#if mode === "side" || mode === "full"}
    <rect
      x={mode === "full" ? "5.9" : "12.6"}
      y="7.4"
      width={mode === "full" ? "12.2" : "5.5"}
      height="9.2"
      rx="0.7"
      fill="currentColor"
      stroke="none"
    />
  {/if}
  {#if mode === "center"}
    <rect x="7" y="9" width="10" height="6" rx="0.7" fill="currentColor" stroke="none" />
  {/if}
</svg>
