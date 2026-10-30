<script lang="ts">
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import SwitchField from "$lib/components/ui/SwitchField.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getSync } from "$lib/stores/sync.svelte";
  import {
    canRequestSync,
    syncClockAheadLines,
    syncConflictLines,
    syncConnectionLabel,
    syncDegradedActionLabel,
    syncDegradedRows,
    type SyncDegradedAction,
    syncDescription,
    syncHeldLines,
    syncLastExchangeLabel,
    syncDeliveryLines,
    syncRecoveryTotal,
    syncRelayLines,
    syncRoleLabel,
    syncStateLabel,
    syncUncountedDeviceLines,
  } from "$lib/sync/status";
  import { syncPresentations } from "$lib/sync/presentations";
  import { openMobileBackgroundExecutionSettings } from "$lib/scheduling/mobile-background-execution";
  import SyncRecoveryList from "./SyncRecoveryList.svelte";
  import SyncDevicesList from "./SyncDevicesList.svelte";
  import SyncLateChanges from "./SyncLateChanges.svelte";
  import SyncOutboxList from "./SyncOutboxList.svelte";
  import SyncRecoveryKit from "./SyncRecoveryKit.svelte";
  import SyncServer from "./SyncServer.svelte";
  import SyncContestCard from "./SyncContestCard.svelte";
  import { syncKitContested, syncStorageAlertLines } from "$lib/sync/contests";
  import ConfirmDialog from "$lib/components/ui/ConfirmDialog.svelte";
  import { formatHandoffError } from "$lib/vault/handoff-workflow";
  import { getSyncAuthorityState } from "$lib/stores/sync-authority.svelte";
  import {
    canInstallSyncRecoveryKey,
    canManageSyncRecoveryKit,
    canManageSyncServer,
    canResetSyncState,
    canResolveSyncLateChanges,
    canSuggestSyncHandover,
    syncAnchorUnreachableLine,
    syncQuarantinableAnchor,
    syncRecoveryKitUse,
    type SyncAuthorityContext,
    syncUnconfirmedNotice,
  } from "$lib/sync/authority";

  let {
    buttonClass,
    mobileLayout = false,
    onAllowNetworkAccess,
    onRecoverLocalCopy,
    onFreshCopy,
  }: {
    buttonClass: string;
    mobileLayout?: boolean;
    /** Starts allowing this desktop's sync listener through the firewall, when this device can. */
    onAllowNetworkAccess?: () => void;
    /** Starts recovering this local copy as a separate vault, when this device can. */
    onRecoverLocalCopy?: () => void;
    /** Copies the linked vault to this device again after sync reported that it must; absent where this device cannot receive one. */
    onFreshCopy?: () => Promise<void>;
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
  const clockAhead = $derived(syncClockAheadLines(t, status));
  const uncountedDevices = $derived(syncUncountedDeviceLines(t, status));
  const conflictLines = $derived(syncConflictLines(t, syncPresentations, status));
  const recoveryTotal = $derived(syncRecoveryTotal(status));
  const description = $derived(syncDescription(t, localization.locale, syncPresentations));
  const stateLabel = $derived(syncStateLabel(t, status.state));
  const connectionLabel = $derived(syncConnectionLabel(t, status));
  const relayLines = $derived(syncRelayLines(t, localization.locale, status, now));
  const deliveryLines = $derived(syncDeliveryLines(t, status));
  const degradedRows = $derived(syncDegradedRows(t, status));
  const authorityState = getSyncAuthorityState();
  let resetConfirming = $state(false);
  let resetting = $state(false);
  let resetMessage = $state("");
  const anchorUnreachable = $derived(syncAnchorUnreachableLine(t, localization.locale, status, now));
  const authorityContext = $derived<SyncAuthorityContext | null>(
    authorityState.authority
      ? {
        authority: authorityState.authority,
        statusRole: status.role,
        mobileLayout,
        anchorUnreachableSinceMs: status.anchorUnreachableSinceMs,
      }
      : null,
  );
  const unconfirmed = $derived(
    syncUnconfirmedNotice(t, localization.locale, status, now, canSuggestSyncHandover(authorityContext)),
  );
  const canManageKit = $derived(authorityContext !== null && canManageSyncRecoveryKit(authorityContext));
  const canInstallKey = $derived(authorityContext !== null && canInstallSyncRecoveryKey(authorityContext));
  const contests = $derived(authorityState.contests);
  const kitUse = $derived(
    authorityContext === null ? null : syncRecoveryKitUse(authorityContext, status.state, syncKitContested(contests)),
  );
  const storageBackstop = $derived(contests?.storage.backstop ?? null);
  const storageAlerts = $derived(contests ? syncStorageAlertLines(t, localization.locale, contests.storage) : []);
  let resuming = $state(false);
  const canReset = $derived(canResetSyncState(status.role, mobileLayout));
  let freshCopyConfirming = $state(false);
  let copyingFresh = $state(false);

  async function getFreshCopy(): Promise<void> {
    freshCopyConfirming = false;
    if (copyingFresh || !onFreshCopy) return;
    copyingFresh = true;
    actionError = "";
    resetMessage = "";
    try {
      await onFreshCopy();
      resetMessage = t("sync.bootstrapCopied");
    } catch (cause: unknown) {
      actionError = formatHandoffError(cause, t);
    } finally {
      copyingFresh = false;
    }
  }

  async function resetSync(): Promise<void> {
    resetConfirming = false;
    if (resetting) return;
    resetting = true;
    actionError = "";
    resetMessage = "";
    try {
      await authorityState.resetSyncState();
      resetMessage = t("sync.reset.done");
    } catch (cause: unknown) {
      console.warn("sync reset failed", cause);
      actionError = t("sync.reset.failed");
    } finally {
      resetting = false;
    }
  }

  /** Whether this section can run `action` here. */
  function canRunDegradedAction(action: SyncDegradedAction): boolean {
    return action === "openBatterySettings" ? mobileLayout : onAllowNetworkAccess !== undefined;
  }

  async function runDegradedAction(action: SyncDegradedAction): Promise<void> {
    if (action === "allowNetworkAccess" || action === "allowPeerConnections") {
      onAllowNetworkAccess?.();
      return;
    }
    try {
      await openMobileBackgroundExecutionSettings("battery");
    } catch (cause: unknown) {
      console.warn("Android battery settings could not be opened", cause);
    }
  }

  /** Resumes the record exchange the storage backstop stopped. */
  async function resumeRecords(): Promise<void> {
    if (resuming) return;
    resuming = true;
    actionError = "";
    try {
      await authorityState.resumeRecords();
    } catch (cause: unknown) {
      console.warn("sync record exchange could not be resumed", cause);
      actionError = t("sync.storage.resumeFailed");
    } finally {
      resuming = false;
    }
  }

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
  $effect(() => authorityState.subscribe());

  $effect(() => {
    const timer = setInterval(() => { now = Date.now(); }, CLOCK_TICK_MS);
    return () => clearInterval(timer);
  });
</script>

<div class="flex min-w-0 flex-col gap-3">
  <div class="px-1 py-1">
    <div class="text-[0.866667rem] text-foreground">{t("sync.heading")}</div>
    <div class="mt-0.5 text-[0.8rem] text-muted-foreground">{description}</div>
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
      {#if connectionLabel}
        <div class="min-w-0 break-words text-muted-foreground">{connectionLabel}</div>
      {/if}
      <div class="text-muted-foreground">{syncLastExchangeLabel(t, localization.locale, status, now)}</div>
      {#if status.pendingLocalChanges > 0}
        <div class="text-muted-foreground">{t("sync.pending", status.pendingLocalChanges)}</div>
      {/if}
      {#if status.waiting > 0}
        <div class="text-muted-foreground">{t("sync.waiting", status.waiting)}</div>
      {/if}
      {#each deliveryLines as line (line)}
        <div class="min-w-0 break-words text-muted-foreground">{line}</div>
      {/each}
      {#each relayLines as line (line)}
        <div class="min-w-0 break-words text-muted-foreground">{line}</div>
      {/each}
      {#if status.state === "error" && status.lastErrorCode === "writerLimit"}
        <Notice message={t("sync.writerLimit")} />
      {:else if status.state === "error" && status.lastErrorCode === "bootstrapRequired"}
        <Notice tone="warning" role="status" copy={false} message={t("sync.bootstrapRequired")}>
          {#snippet actions()}
            {#if onFreshCopy}
              <button
                type="button"
                class="notice-action-text"
                disabled={copyingFresh}
                onclick={() => { freshCopyConfirming = true; }}
              >
                {t("sync.bootstrapAction")}
              </button>
            {/if}
          {/snippet}
        </Notice>
      {:else if status.lastErrorCode === "coordinatorRemoved" && onRecoverLocalCopy}
        <Notice tone="warning" role="status" copy={false} message={t("sync.coordinatorRemoved")}>
          {#snippet actions()}
            <button type="button" class="notice-action-text" onclick={onRecoverLocalCopy}>
              {t("vaultHandoff.recover")}
            </button>
          {/snippet}
        </Notice>
      {:else if status.lastErrorCode === "coordinatorRemoved"}
        <Notice tone="warning" role="status" copy={false} message={t("sync.coordinatorRemoved")} />
      {:else if status.state === "error" && status.lastError}
        <Notice message={t("sync.errorDetail", status.lastError)} />
      {/if}
      {#each degradedRows as row (row.reason)}
        {#if row.action && canRunDegradedAction(row.action)}
          {@const action = row.action}
          <Notice tone="warning" role="status" copy={false} message={row.label}>
            {#snippet actions()}
              <button type="button" class="notice-action-text" onclick={() => void runDegradedAction(action)}>
                {syncDegradedActionLabel(t, action)}
              </button>
            {/snippet}
          </Notice>
        {:else}
          <Notice tone="warning" role="status" copy={false} message={row.label} />
        {/if}
      {/each}
      {#each clockAhead as line (line)}<Notice message={line} />{/each}
      {#each uncountedDevices as entry (entry.deviceId)}
        <Notice tone="warning" role="status" copy={false} message={entry.line} />
      {/each}
      {#each conflictLines as entry (entry.table)}<div class="text-foreground">{entry.line}</div>{/each}
      {#if unconfirmed}
        <Notice
          tone={unconfirmed.suggestHandover ? "warning" : "neutral"}
          role="status"
          copy={false}
          message={unconfirmed.lines.join(" ")}
        />
      {/if}
      {#if anchorUnreachable}
        <Notice tone="warning" role="status" copy={false} message={anchorUnreachable} />
      {/if}
      {#if storageBackstop}
        <Notice
          tone="warning"
          role="status"
          copy={false}
          message={`${storageBackstop === "reserve" ? t("sync.storage.reserve") : t("sync.storage.stopped")} ${t("sync.storage.resumeHint")}`}
        >
          {#snippet actions()}
            <button type="button" class="notice-action-text" disabled={resuming} onclick={() => void resumeRecords()}>
              {t("sync.storage.resume")}
            </button>
          {/snippet}
        </Notice>
      {/if}
      {#each storageAlerts as line (line)}
        <Notice tone="warning" role="status" copy={false} message={line} />
      {/each}
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
      {#if canReset}
        <button type="button" class={buttonClass} disabled={resetting} onclick={() => { resetConfirming = true; }}>
          {t("sync.reset.action")}
        </button>
      {/if}
    </div>
    {#if actionError}
      <Notice class="px-1 text-[0.8rem] leading-5" message={actionError} />
    {:else if resetMessage}
      <p role="status" class="px-1 text-[0.8rem] leading-5 text-muted-foreground">{resetMessage}</p>
    {/if}

    {#if recoveryTotal > 0}
      <SyncRecoveryList {buttonClass} {mobileLayout} recoveryCount={recoveryTotal} {now} />
    {/if}

    {#if authorityState.loadFailed && !authorityContext}
      <Notice class="px-1 text-[0.8rem] leading-5" message={t("sync.authority.loadFailed")} copy={false} onRetry={authorityState.refresh} />
    {/if}
    {#if authorityContext}
      {#each contests?.contests ?? [] as contest (contest.rootHash)}
        <SyncContestCard buttonClass={mobileLayout ? `${buttonClass} min-h-12` : buttonClass} {contest} />
      {/each}
      <SyncDevicesList {buttonClass} {mobileLayout} context={authorityContext} />
      {#if authorityState.outbox.length > 0}
        <SyncOutboxList entries={authorityState.outbox} {now} />
      {/if}
      {#if authorityState.lateChanges.length > 0}
        <SyncLateChanges
          buttonClass={mobileLayout ? `${buttonClass} min-h-12` : buttonClass}
          changes={authorityState.lateChanges}
          total={authorityState.lateTotal}
          canResolve={canResolveSyncLateChanges(authorityContext)}
          {now}
        />
      {/if}
      {#if canManageKit || kitUse || !mobileLayout}
        <SyncRecoveryKit
          {buttonClass}
          canManage={canManageKit}
          {canInstallKey}
          use={kitUse}
          anchor={syncQuarantinableAnchor(authorityContext.authority)}
          pendingKit={authorityState.pendingKit}
          {contests}
        />
      {/if}
    {/if}
    <SyncServer
      buttonClass={mobileLayout ? `${buttonClass} min-h-12` : buttonClass}
      canManage={authorityContext !== null && canManageSyncServer(authorityContext)}
    />
  {/if}
</div>

{#if freshCopyConfirming}
  <ConfirmDialog
    title={t("sync.bootstrapConfirmTitle")}
    message={t("sync.bootstrapConfirmMessage")}
    confirmLabel={t("sync.bootstrapAction")}
    cancelLabel={t("common.cancel")}
    danger={false}
    onConfirm={() => void getFreshCopy()}
    onCancel={() => { freshCopyConfirming = false; }}
  />
{/if}

{#if resetConfirming}
  <ConfirmDialog
    title={t("sync.reset.title")}
    message={t("sync.reset.message")}
    confirmLabel={t("sync.reset.action")}
    cancelLabel={t("common.cancel")}
    onConfirm={() => void resetSync()}
    onCancel={() => { resetConfirming = false; }}
  />
{/if}
