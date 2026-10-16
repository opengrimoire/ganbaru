<script lang="ts">
  import Archive from "@lucide/svelte/icons/archive";
  import ArchiveRestore from "@lucide/svelte/icons/archive-restore";
  import ArrowLeft from "@lucide/svelte/icons/arrow-left";
  import Hash from "@lucide/svelte/icons/hash";
  import Search from "@lucide/svelte/icons/search";
  import type { ChatChannelRead } from "$lib/chat/contracts";
  import { formatDateTime } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { useChat } from "$lib/stores/chat.svelte";
  import Loading from "$lib/components/ui/Loading.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";

  const localization = getLocalization();
  const { t } = localization;
  const chat = useChat();
  let query = $state("");
  let restoringId = $state<string | null>(null);
  let error = $state<string | null>(null);
  const filteredChannels = $derived(chat.archivedChannels.filter((channel) => {
    const normalized = query.trim().toLocaleLowerCase();
    return !normalized
      || channel.name.toLocaleLowerCase().includes(normalized)
      || channel.topic.toLocaleLowerCase().includes(normalized);
  }));

  async function restore(channel: ChatChannelRead): Promise<void> {
    restoringId = channel.id;
    error = null;
    try {
      await chat.restoreChannel(channel);
    } catch (cause: unknown) {
      error = cause instanceof Error ? cause.message : String(cause);
    } finally {
      restoringId = null;
    }
  }
</script>

<section class="flex min-w-0 flex-1 flex-col overflow-hidden">
  <div class="flex shrink-0 items-center gap-2 border-b border-border px-4 py-3 sm:px-6">
    <button type="button" class="rounded-md p-1.5 text-muted-foreground hover:bg-accent hover:text-foreground" aria-label={t("chat.channels.backToChannel")} onclick={() => chat.closeChannelArchive()}><ArrowLeft size={16} /></button>
    <Archive size={16} class="text-muted-foreground" />
    <h2 class="min-w-0 flex-1 truncate text-[1.05rem] font-semibold">{t("chat.channels.archive")}</h2>
  </div>
  <div class="shrink-0 px-4 py-3 sm:px-6">
    <label class="field flex max-w-xl items-center gap-1.5 px-2 py-1.5"><Search size={16} class="text-muted-foreground" /><input class="field-bare text-[0.866667rem]" type="search" bind:value={query} placeholder={t("chat.channels.searchArchive")} aria-label={t("chat.channels.searchArchive")} /></label>
  </div>
  <div class="min-h-0 flex-1 overflow-auto px-4 pb-5 sm:px-6">
    {#if error}<Notice class="py-2 text-sm" message={error} />{/if}
    {#if chat.archivedChannelsLoading}
      <Loading layout="rows" rows={3} class="max-w-3xl py-2" />
    {:else if chat.archivedChannelsError}
      <Notice class="py-2 text-sm" message={chat.archivedChannelsError} />
    {:else if filteredChannels.length === 0}
      <p class="py-2 text-sm text-muted-foreground">{query.trim() ? t("chat.channels.noArchiveResults") : t("chat.channels.emptyArchive")}</p>
    {:else}
      <div class="flex max-w-3xl flex-col gap-1">
        {#each filteredChannels as channel (channel.id)}
          <div class="flex min-w-0 flex-wrap items-start gap-3 rounded-md px-2 py-2 hover:bg-accent/70">
            <Hash size={16} class="mt-0.5 shrink-0 text-muted-foreground" />
            <div class="min-w-32 flex-1"><div class="truncate text-sm font-medium">{channel.name}</div><div class="mt-0.5 truncate text-xs text-muted-foreground">{channel.topic || t("chat.channels.noTopic")} · {formatDateTime(localization.locale, Date.parse(channel.archivedAt ?? channel.updatedAt), { dateStyle: "medium", timeStyle: "short" })}</div></div>
            <button type="button" class="flex shrink-0 items-center gap-1.5 rounded-md border border-border bg-background px-2 py-1.5 text-xs hover:bg-accent disabled:opacity-60" disabled={restoringId === channel.id} onclick={() => void restore(channel)}><ArchiveRestore size={15} />{t("chat.restore")}</button>
          </div>
        {/each}
      </div>
    {/if}
  </div>
</section>
