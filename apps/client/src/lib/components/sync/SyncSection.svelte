<script lang="ts">
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import SwitchField from "$lib/components/ui/SwitchField.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getSync } from "$lib/stores/sync.svelte";
  import {
    canRequestSync,
    syncConflictTotal,
    syncHeldLines,
    syncLastExchangeLabel,
    syncRoleLabel,
    syncStateLabel,
  } from "$lib/sync/status";
  import SyncRecoveryList from "./SyncRecoveryList.svelte";

  let {
    buttonClass,
    mobileLayout = false,
  }: {
    buttonClass: string;
    mobileLayout?: boolean;
  } = $props();

  const CLOCK_TICK_MS = 30_000;
  const localization = getLocalization();
  const { t } = localization;
  const sync = getSync();
  let now = $state(Date.now());
  let requesting = $state(false);
  let pausing = $state(false);
  let actionError = $state("");

  const status = $derived(sync.status);
  const held = $derived(syncHeldLines(t, status));
  const conflictTotal = $derived(syncConflictTotal(status));
  const stateLabel = $derived(syncStateLabel(t, status.state));

  async function requestSync(): Promise<void> {
    if (requesting) return;
    requesting = true;
    actionError = "";
    try {
      await sync.requestSync();
    } catch (cause: unknown) {
      console.warn("sync request failed", cause);
      actionError = t("sync.syncNowFailed");
    } finally {
      requesting = false;
    }
  }

  async function setPaused(paused: boolean): Promise<void> {
    if (pausing) return;
    pausing = true;
    actionError = "";
    try {
      await sync.setPaused(paused);
    } catch (cause: unknown) {
      console.warn("sync pause change failed", cause);
      actionError = t("sync.pauseFailed");
    } finally {
      pausing = false;
    }
  }

  $effect(() => sync.subscribe());

  $effect(() => {
    const timer = setInterval(() => { now = Date.now(); }, CLOCK_TICK_MS);
    return () => clearInterval(timer);
  });
</script>

<div class="flex min-w-0 flex-col gap-3">
  <div class="px-1 py-1">
    <div class="text-[0.866667rem] text-foreground">{t("sync.heading")}</div>
    <div class="mt-0.5 text-[0.8rem] text-muted-foreground">{t("sync.description")}</div>
  </div>

  {#if status.state !== "off"}
    <div class="flex flex-col gap-1 px-1 text-[0.8rem] leading-5" role="status">
      <div class="flex items-center gap-2 text-foreground">
        {#if status.state === "syncing"}
          <LoaderCircle size={14} strokeWidth={2} class="shrink-0 animate-spin" aria-hidden="true" />
        {/if}
        <span class="min-w-0">{stateLabel}</span>
      </div>
      {#if status.role}
        <div class="text-muted-foreground">{syncRoleLabel(t, status.role)}</div>
      {/if}
      <div class="text-muted-foreground">{syncLastExchangeLabel(t, localization.locale, status, now)}</div>
      {#if status.pendingLocalChanges > 0}
        <div class="text-muted-foreground">{t("sync.pending", status.pendingLocalChanges)}</div>
      {/if}
      {#if status.waiting > 0}
        <div class="text-muted-foreground">{t("sync.waiting", status.waiting)}</div>
      {/if}
      {#if status.state === "error" && status.lastErrorCode === "writerLimit"}
        <Notice message={t("sync.writerLimit")} />
      {:else if status.state === "error" && status.lastError}
        <Notice message={t("sync.errorDetail", status.lastError)} />
      {/if}
      {#if conflictTotal > 0}
        <div class="text-foreground">{t("sync.conflicts", conflictTotal)}</div>
      {/if}
    </div>

    {#if held.length > 0}
      <div class="flex flex-col gap-1 px-1 text-[0.8rem] leading-5">
        <div class="text-foreground">{t("sync.heldHeading")}</div>
        <ul class="flex flex-col gap-0.5 text-muted-foreground">
          {#each held as line (line)}<li>{line}</li>{/each}
        </ul>
      </div>
    {/if}

    <SwitchField
      label={t("sync.pause")}
      checked={status.paused}
      disabled={pausing}
      onChange={(paused) => void setPaused(paused)}
    />

    <div class="flex flex-wrap gap-2">
      <button
        type="button"
        class={mobileLayout ? `${buttonClass} min-h-12` : buttonClass}
        disabled={requesting || !canRequestSync(status)}
        onclick={() => void requestSync()}
      >
        <RefreshCw size={14} strokeWidth={1.8} class={requesting ? "animate-spin" : ""} aria-hidden="true" />
        {t("sync.syncNow")}
      </button>
    </div>
    {#if actionError}
      <Notice class="px-1 text-[0.8rem] leading-5" message={actionError} />
    {/if}

    {#if status.recoveryCount > 0}
      <SyncRecoveryList {buttonClass} {mobileLayout} recoveryCount={status.recoveryCount} {now} />
    {/if}
  {/if}
</div>
