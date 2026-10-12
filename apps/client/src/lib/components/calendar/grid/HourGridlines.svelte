<script lang="ts">
  const hourLines = Array.from({ length: 23 }, (_, i) => i + 1);
</script>

<div
  class="pointer-events-none absolute inset-x-0 top-0 z-1"
  style="height: calc(24 * var(--hour-h) * 1px);"
  aria-hidden="true"
>
  {#each hourLines as hour}
    <div
      class="absolute left-0 right-0"
      style="top: calc({hour} * var(--hour-h) * 1px); height: 0; border-bottom: 1px solid var(--cal-gridline);"
    ></div>
  {/each}
</div>
