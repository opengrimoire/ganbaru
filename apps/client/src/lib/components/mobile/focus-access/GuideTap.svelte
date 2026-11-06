<script lang="ts">
  /**
   * A fingertip press drawn over its parent's center, which must be positioned. It appears when `active` turns on and
   * holds until it turns off.
   */
  let { active }: { active: boolean } = $props();
</script>

{#if active}
  <span class="guide-tap pointer-events-none absolute left-1/2 top-1/2 size-7 rounded-full bg-foreground/25 ring-2 ring-foreground/40"></span>
{/if}

<style>
  .guide-tap {
    translate: -50% -50%;
    animation: guide-tap 650ms cubic-bezier(0.2, 0, 0, 1) both;
  }

  @keyframes guide-tap {
    0% { opacity: 0; scale: 1.6; }
    45% { opacity: 1; scale: 0.85; }
    100% { opacity: 1; scale: 1; }
  }

  @media (prefers-reduced-motion: reduce) {
    .guide-tap { animation: none; }
  }
</style>
