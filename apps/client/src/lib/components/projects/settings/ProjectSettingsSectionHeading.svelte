<script lang="ts">
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { formatNumber } from "$lib/i18n/formatters";

  let {
    label,
    count = null,
    inlineCount = false,
  }: {
    label: string;
    count?: string | number | null;
    inlineCount?: boolean;
  } = $props();
  const localization = getLocalization();
</script>

<div class="flex min-h-7 items-center {inlineCount ? 'gap-2' : 'justify-between gap-3 px-1'}">
  <h2 class="truncate text-[0.8rem] font-medium text-foreground">{label}</h2>
  {#if count}
    <span class="shrink-0 text-[0.733333rem] text-muted-foreground {inlineCount ? 'tabular-nums' : 'font-medium'}">{typeof count === "number" ? formatNumber(localization.locale, count) : count}</span>
  {/if}
</div>
