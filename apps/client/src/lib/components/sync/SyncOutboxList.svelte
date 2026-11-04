<script lang="ts">
  import type { SyncOutboxEntry } from "$lib/api/sync";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { syncOutboxChangeLabel, syncOutboxStateLine } from "$lib/sync/authority";
  import { formatSyncTime } from "$lib/sync/status";

  let {
    entries,
    now,
  }: {
    entries: SyncOutboxEntry[];
    now: number;
  } = $props();

  const localization = getLocalization();
  const { t } = localization;
</script>

<div class="flex min-w-0 flex-col gap-2">
  <div class="px-1">
    <div class="text-[0.866667rem] text-foreground">{t("sync.outbox.heading")}</div>
    <div class="mt-0.5 text-[0.8rem] text-muted-foreground">{t("sync.outbox.description")}</div>
  </div>
  <ul class="flex min-w-0 flex-col gap-2 px-1">
    {#each entries as entry, index (`${entry.createdAtMs}:${entry.change}:${index}`)}
      <li class="min-w-0">
        <div class="truncate text-[0.866667rem] text-foreground">{syncOutboxChangeLabel(t, entry)}</div>
        <div class={`text-[0.75rem] ${entry.state === "rejected" ? "text-destructive" : "text-muted-foreground"}`}>
          {syncOutboxStateLine(t, entry)}
        </div>
        <div class="text-[0.75rem] text-muted-foreground">{formatSyncTime(t, localization.locale, entry.createdAtMs, now)}</div>
      </li>
    {/each}
  </ul>
</div>
