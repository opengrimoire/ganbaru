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
    syncHeldLines,
    syncLastExchangeLabel,
    syncDeliveryLines,
    syncDeviceAcknowledgementLines,
    syncRecoveryTotal,
    syncErrorMessage,
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
    IDLE_SYNC_ACTIONS,
    runSyncAction,
    syncActionRunning,
    type SyncActionAccess,
    type SyncActionState,
  } from "./sync-action-state";
  import {
    canInstallSyncRecoveryKey,
    canManageSyncRecoveryKit,
    canRotateSyncRecoveryKit,
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
    onRecoverLocalCopy,
    onFreshCopy,
  }: {
    buttonClass: string;
    mobileLayout?: boolean;
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
  /** The section actions, which run side by side and share one feedback line. */
  type SyncSectionAction = "freshCopy" | "reset" | "resume" | "syncNow" | "pause";
  let actions = $state<SyncActionState<SyncSectionAction>>(IDLE_SYNC_ACTIONS);
  const actionAccess: SyncActionAccess<SyncSectionAction> = { read: () => actions, write: (next) => { actions = next; } };
  const requesting = $derived(syncActionRunning(actions, "syncNow"));
  const pausing = $derived(syncActionRunning(actions, "pause"));

  const status = $derived(sync.status);
  const held = $derived(syncHeldLines(t, status));
  const clockAhead = $derived(syncClockAheadLines(t, status));
  const uncountedDevices = $derived(syncUncountedDeviceLines(t, status));
  const acknowledgementLines = $derived(syncDeviceAcknowledgementLines(t, localization.locale, status, now));
  const conflictLines = $derived(syncConflictLines(t, syncPresentations, status));
  const recoveryTotal = $derived(syncRecoveryTotal(status));
  const stateLabel = $derived(syncStateLabel(t, status.state));
  const connectionLabel = $derived(syncConnectionLabel(t, status));
  const relayLines = $derived(syncRelayLines(t, localization.locale, status, now));
  const deliveryLines = $derived(syncDeliveryLines(t, status));
  const degradedRows = $derived(syncDegradedRows(t, status));
  const authorityState = getSyncAuthorityState();
  let resetConfirming = $state(false);
  const resetting = $derived(syncActionRunning(actions, "reset"));
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
  const canRotateKit = $derived(authorityContext !== null && canRotateSyncRecoveryKit(authorityContext));
  const contests = $derived(authorityState.contests);
  const kitUse = $derived(
    authorityContext === null ? null : syncRecoveryKitUse(authorityContext, status.state, syncKitContested(contests)),
  );
  const storageBackstop = $derived(contests?.storage.backstop ?? null);
  const storageAlerts = $derived(contests ? syncStorageAlertLines(t, localization.locale, contests.storage) : []);
  const resuming = $derived(syncActionRunning(actions, "resume"));
  const canReset = $derived(canResetSyncState(status.role, mobileLayout));
  let freshCopyConfirming = $state(false);
  const copyingFresh = $derived(syncActionRunning(actions, "freshCopy"));

  async function getFreshCopy(): Promise<void> {
    freshCopyConfirming = false;
    const copy = onFreshCopy;
    if (!copy) return;
    await runSyncAction(actionAccess, "freshCopy", copy, {
      success: () => t("sync.bootstrapCopied"),
      failure: (cause) => formatHandoffError(cause, t),
    });
  }

  async function resetSync(): Promise<void> {
    resetConfirming = false;
    await runSyncAction(actionAccess, "reset", () => authorityState.resetSyncState(), {
      success: () => t("sync.reset.done"),
      failure: (cause) => {
        console.warn("sync reset failed", cause);
        return t("sync.reset.failed");
      },
    });
  }

  /** Whether this section can run `action` here: battery settings open only on Android. */
  function canRunDegradedAction(action: SyncDegradedAction): boolean {
    return action === "openBatterySettings" && mobileLayout;
  }

  async function runDegradedAction(): Promise<void> {
    try {
      await openMobileBackgroundExecutionSettings("battery");
    } catch (cause: unknown) {
      console.warn("Android battery settings could not be opened", cause);
    }
  }

  /** Resumes the record exchange the storage backstop stopped. */
  async function resumeRecords(): Promise<void> {
    await runSyncAction(actionAccess, "resume", () => authorityState.resumeRecords(), {
      failure: (cause) => {
        console.warn("sync record exchange could not be resumed", cause);
        return t("sync.storage.resumeFailed");
      },
    });
  }

  async function requestSync(): Promise<void> {
    await runSyncAction(actionAccess, "syncNow", () => sync.requestSync(), {
      failure: (cause) => {
        console.warn("sync request failed", cause);
        return t("sync.syncNowFailed");
      },
    });
  }

  async function setPaused(paused: boolean): Promise<void> {
    await runSyncAction(actionAccess, "pause", () => sync.setPaused(paused), {
      failure: (cause) => {
        console.warn("sync pause change failed", cause);
        return t("sync.pauseFailed");
      },
    });
  }

  $effect(() => sync.subscribe());
  $effect(() => authorityState.subscribe());

  $effect(() => {
    const timer = setInterval(() => { now = Date.now(); }, CLOCK_TICK_MS);
    return () => clearInterval(timer);
  });
</script>

<div class="flex min-w-0 flex-col gap-3">
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
      {#each acknowledgementLines as entry (entry.deviceId)}
        <div class="min-w-0 break-words text-muted-foreground">{entry.line}</div>
      {/each}
      {#if status.state === "error" && status.lastErrorCode === "writerLimit"}
        <Notice message={t("sync.writerLimit")} />
      {:else if status.state === "error" && status.lastErrorCode === "forkClaimUnproven"}
        <Notice tone="warning" role="status" copy={false} message={t("sync.forkClaimUnproven")} />
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
        <Notice message={syncErrorMessage(t, status.lastErrorCode)} copy={status.lastError} />
      {/if}
      {#each degradedRows as row (row.reason)}
        {#if row.action && canRunDegradedAction(row.action)}
          {@const action = row.action}
          <Notice tone="warning" role="status" copy={false} message={row.label}>
            {#snippet actions()}
              <button type="button" class="notice-action-text" onclick={() => void runDegradedAction()}>
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
    {#if actions.feedback.kind === "failed"}
      <Notice class="px-1 text-[0.8rem] leading-5" message={actions.feedback.message} />
    {:else if actions.feedback.kind === "done"}
      <p role="status" class="px-1 text-[0.8rem] leading-5 text-muted-foreground">{actions.feedback.message}</p>
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
          canRotate={canRotateKit}
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
