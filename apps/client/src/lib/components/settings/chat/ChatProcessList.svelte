<script lang="ts">
  import { onMount, untrack } from "svelte";
  import RotateCw from "@lucide/svelte/icons/rotate-cw";
  import * as chatApi from "$lib/api/chat";
  import type { ChatProcessListRead, ChatProcessPurpose, ChatProcessRecord } from "$lib/chat/contracts";
  import { requestConfirmation } from "$lib/components/ui/confirmation.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { formatDateTime, formatNumber } from "$lib/i18n/formatters";
  import { getLocalization, type TranslationKey } from "$lib/i18n/translator.svelte";

  /** Lists the running Chat processes on this device and stops one process group after confirmation. */
  let { refreshKey = 0 }: { refreshKey?: number } = $props();

  const localization = getLocalization();
  const { t } = localization;
  let processes = $state<ChatProcessListRead | null>(null);
  let busy = $state(false);
  let error = $state<string | null>(null);
  let status = $state<string | null>(null);

  const PURPOSE_LABELS: Record<ChatProcessPurpose, TranslationKey> = {
    session: "settings.chat.behavior.processPurposeSession",
    probe: "settings.chat.behavior.processPurposeProbe",
    local_server: "settings.chat.behavior.processPurposeLocalServer",
    terminal: "settings.chat.behavior.processPurposeTerminal",
  };

  function errorMessage(cause: unknown): string {
    return cause instanceof Error ? cause.message : String(cause);
  }

  function pidLabel(pid: number): string {
    return formatNumber(localization.locale, pid, { useGrouping: false });
  }

  function startedLabel(record: ChatProcessRecord): string {
    return formatDateTime(localization.locale, new Date(record.startedAt), { dateStyle: "short", timeStyle: "short" });
  }

  async function refresh(): Promise<void> {
    if (busy) return;
    busy = true;
    error = null;
    try {
      processes = await chatApi.listChatProcesses();
    } catch (cause: unknown) {
      error = errorMessage(cause);
    } finally {
      busy = false;
    }
  }

  async function stop(record: ChatProcessRecord): Promise<void> {
    if (busy || record.startTime === null) return;
    const confirmed = await requestConfirmation({
      message: t("settings.chat.behavior.stopProcessConfirm", record.executableName, pidLabel(record.pid)),
      confirmLabel: t("settings.chat.behavior.stopProcess"),
      danger: true,
    });
    if (!confirmed) return;
    busy = true;
    error = null;
    status = null;
    try {
      await chatApi.stopChatProcess(record.pid, record.startTime);
      status = t("settings.chat.behavior.processStopped", record.executableName);
    } catch (cause: unknown) {
      error = errorMessage(cause);
    } finally {
      busy = false;
    }
    await refresh();
  }

  onMount(() => {
    void refresh();
  });

  $effect(() => {
    if (refreshKey > 0) untrack(() => void refresh());
  });
</script>

<div class="space-y-2">
  <div class="flex items-center justify-between gap-2">
    <h4 class="text-sm font-medium">{t("settings.chat.behavior.processList")}</h4>
    <button type="button" class="process-action inline-flex items-center gap-1" disabled={busy} onclick={() => void refresh()}><RotateCw class="size-3.5" aria-hidden="true" />{t("settings.chat.behavior.refreshProcesses")}</button>
  </div>
  <p class="text-xs text-muted-foreground">{t("settings.chat.behavior.processesDescription")}</p>
  {#if error}<Notice class="text-sm" message={error} />{/if}
  {#if status}<p role="status" class="text-sm text-action-confirm">{status}</p>{/if}
  {#if processes}
    {#if processes.roots.length === 0}
      <p class="text-xs text-muted-foreground">{t("settings.chat.behavior.processesEmpty")}</p>
    {:else}
      <ul class="space-y-2">
        {#each processes.roots as record (`${record.pid}:${record.startTime ?? "unknown"}`)}
          {@const children = processes.descendants.filter((child) => child.rootPid === record.pid)}
          <li class="space-y-1">
            <div class="flex min-w-0 items-center gap-2">
              <div class="min-w-0 flex-1">
                <p class="truncate text-sm">{record.executableName}</p>
                <p class="truncate text-xs text-muted-foreground">{[t(PURPOSE_LABELS[record.purpose]), record.providerInstanceLabel, t("settings.chat.behavior.processPid", pidLabel(record.pid)), t("settings.chat.behavior.processStartedAt", startedLabel(record))].filter(Boolean).join(" · ")}</p>
              </div>
              {#if record.stoppable}
                <button type="button" class="process-action shrink-0" disabled={busy} aria-label={t("settings.chat.behavior.stopProcessLabel", record.executableName, pidLabel(record.pid))} onclick={() => void stop(record)}>{t("settings.chat.behavior.stopProcess")}</button>
              {/if}
            </div>
            {#if children.length > 0}
              <ul class="space-y-0.5 border-l border-border pl-3">
                {#each children as child (child.pid)}
                  <li class="truncate text-xs text-muted-foreground">{child.executableName} · {t("settings.chat.behavior.processPid", pidLabel(child.pid))}</li>
                {/each}
              </ul>
            {/if}
          </li>
        {/each}
      </ul>
    {/if}
    {#if processes.omitted > 0}
      <p class="text-xs text-muted-foreground">{t("settings.chat.behavior.processDescendantsOmitted", formatNumber(localization.locale, processes.omitted))}</p>
    {/if}
  {/if}
</div>

<style>
  .process-action { border: 1px solid var(--border); border-radius: 0.375rem; padding: 0.35rem 0.65rem; font-size: calc(0.733333rem * var(--type-scale)); }
  .process-action:disabled { cursor: not-allowed; opacity: 0.5; }
</style>
