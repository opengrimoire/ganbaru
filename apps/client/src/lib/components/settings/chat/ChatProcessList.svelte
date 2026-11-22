<script lang="ts">
  import { onMount, untrack } from "svelte";
  import RotateCw from "@lucide/svelte/icons/rotate-cw";
  import * as chatApi from "$lib/api/chat";
  import type { ChatProcessListRead, ChatProcessPurpose, ChatProcessRecord } from "$lib/chat/contracts";
  import { requestConfirmation } from "$lib/components/ui/confirmation.svelte";
  import { SETTINGS_ICON_BUTTON_CLASS, SETTINGS_SECONDARY_BUTTON_CLASS } from "$lib/components/settings/action-buttons";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { formatDateTime, formatNumber } from "$lib/i18n/formatters";
  import { getLocalization, type TranslationKey } from "$lib/i18n/translator.svelte";
  import { chatErrorMessage } from "$lib/chat/error-presentation";
  import {
    asyncResourceError,
    failAsyncRequest,
    idleAsyncResource,
    isAsyncResourceLoading,
    resolveAsyncRequest,
    startAsyncRequest,
    type AsyncResource,
  } from "$lib/utils/async-resource";

  /** Lists the running Chat processes on this device and stops one process group after confirmation. */
  let { refreshKey = 0 }: { refreshKey?: number } = $props();

  const localization = getLocalization();
  const { t } = localization;
  /** The process list; a newer refresh supersedes one in flight, so a requested refresh is never dropped. */
  let list = $state.raw<AsyncResource<ChatProcessListRead | null>>(idleAsyncResource(null));
  /** The stop in flight, or the outcome of the last stop. */
  let stopState = $state.raw<StopState>({ phase: "idle" });
  const processes = $derived(list.data);
  const busy = $derived(isAsyncResourceLoading(list) || stopState.phase === "stopping");
  const error = $derived(stopState.phase === "failed" ? stopState.error : asyncResourceError(list));
  const status = $derived(stopState.phase === "stopped" ? stopState.message : null);

  type StopState =
    | { readonly phase: "idle" }
    | { readonly phase: "stopping" }
    | { readonly phase: "stopped"; readonly message: string }
    | { readonly phase: "failed"; readonly error: string };

  const PURPOSE_LABELS: Record<ChatProcessPurpose, TranslationKey> = {
    session: "settings.chat.behavior.processPurposeSession",
    probe: "settings.chat.behavior.processPurposeProbe",
    local_server: "settings.chat.behavior.processPurposeLocalServer",
    terminal: "settings.chat.behavior.processPurposeTerminal",
  };

  function errorMessage(cause: unknown): string {
    return chatErrorMessage(cause, t("chatErrors.actions.settingsAction"));
  }

  function pidLabel(pid: number): string {
    return formatNumber(localization.locale, pid, { useGrouping: false });
  }

  function startedLabel(record: ChatProcessRecord): string {
    return formatDateTime(localization.locale, new Date(record.startedAt), { dateStyle: "short", timeStyle: "short" });
  }

  async function refresh(): Promise<void> {
    const started = startAsyncRequest(list);
    list = started.resource;
    try {
      const next = await chatApi.listChatProcesses();
      list = resolveAsyncRequest(list, started.token, next);
    } catch (cause: unknown) {
      list = failAsyncRequest(list, started.token, errorMessage(cause));
    }
  }

  /** Refreshes on request, dismissing the error of an earlier stop. */
  function refreshByUser(): void {
    if (stopState.phase === "failed") stopState = { phase: "idle" };
    void refresh();
  }

  async function stop(record: ChatProcessRecord): Promise<void> {
    const startTime = record.startTime;
    if (busy || startTime === null) return;
    const confirmed = await requestConfirmation({
      message: t("settings.chat.behavior.stopProcessConfirm", record.executableName, pidLabel(record.pid)),
      confirmLabel: t("settings.chat.behavior.stopProcess"),
      danger: true,
    });
    if (!confirmed || busy) return;
    stopState = { phase: "stopping" };
    try {
      await chatApi.stopChatProcess(record.pid, startTime);
      stopState = { phase: "stopped", message: t("settings.chat.behavior.processStopped", record.executableName) };
    } catch (cause: unknown) {
      stopState = { phase: "failed", error: errorMessage(cause) };
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

<section class="flex flex-col gap-4">
  <div class="flex items-center justify-between gap-4 px-1">
    <div class="min-w-0 flex-1">
      <h2 class="text-[0.866667rem] font-semibold text-foreground">{t("settings.chat.behavior.processList")}</h2>
      <p class="mt-0.5 text-[0.8rem] text-muted-foreground">{t("settings.chat.behavior.processesDescription")}</p>
    </div>
    <button type="button" class={SETTINGS_ICON_BUTTON_CLASS} disabled={busy} aria-label={t("settings.chat.behavior.refreshProcesses")} title={t("settings.chat.behavior.refreshProcesses")} onclick={refreshByUser}><RotateCw size={13} aria-hidden="true" /></button>
  </div>
  {#if error}<Notice class="px-1 text-[0.733333rem]" message={error} />{/if}
  {#if status}<p role="status" class="px-1 text-[0.8rem] text-action-confirm-signal">{status}</p>{/if}
  {#if processes}
    {#if processes.roots.length === 0}
      <p class="px-1 text-[0.8rem] text-muted-foreground">{t("settings.chat.behavior.processesEmpty")}</p>
    {:else}
      <ul class="flex flex-col gap-2 px-1">
        {#each processes.roots as record (`${record.pid}:${record.startTime ?? "unknown"}`)}
          {@const children = processes.descendants.filter((child) => child.rootPid === record.pid)}
          <li class="space-y-1">
            <div class="flex min-w-0 items-center gap-2">
              <div class="min-w-0 flex-1">
                <p class="truncate text-[0.866667rem] text-foreground">{record.executableName}</p>
                <p class="truncate text-[0.733333rem] text-muted-foreground">{[t(PURPOSE_LABELS[record.purpose]), record.providerInstanceLabel, t("settings.chat.behavior.processPid", pidLabel(record.pid)), t("settings.chat.behavior.processStartedAt", startedLabel(record)), record.resourceScope === "scoped" ? t("settings.chat.behavior.processResourceScoped") : null].filter(Boolean).join(" · ")}</p>
              </div>
              {#if record.stoppable}
                <button type="button" class={SETTINGS_SECONDARY_BUTTON_CLASS} disabled={busy} aria-label={t("settings.chat.behavior.stopProcessLabel", record.executableName, pidLabel(record.pid))} onclick={() => void stop(record)}>{t("settings.chat.behavior.stopProcess")}</button>
              {/if}
            </div>
            {#if children.length > 0}
              <ul class="space-y-0.5 border-l border-border pl-3">
                {#each children as child (child.pid)}
                  <li class="truncate text-[0.733333rem] text-muted-foreground">{child.executableName} · {t("settings.chat.behavior.processPid", pidLabel(child.pid))}</li>
                {/each}
              </ul>
            {/if}
          </li>
        {/each}
      </ul>
    {/if}
    {#if processes.omitted > 0}
      <p class="px-1 text-[0.733333rem] text-muted-foreground">{t("settings.chat.behavior.processDescendantsOmitted", formatNumber(localization.locale, processes.omitted))}</p>
    {/if}
  {/if}
</section>
