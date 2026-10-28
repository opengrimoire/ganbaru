<script lang="ts">
  import type { SyncAuthorityDevice } from "$lib/api/sync";
  import Checkbox from "$lib/components/ui/Checkbox.svelte";
  import ConfirmDialog from "$lib/components/ui/ConfirmDialog.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getSyncAuthorityState } from "$lib/stores/sync-authority.svelte";
  import {
    canForceSyncAnchor,
    canTakeSyncAnchor,
    type SyncAuthorityContext,
    syncAuthorityDeviceName,
    syncDeviceActions,
    syncDeviceBadgeLabel,
    syncDeviceBadges,
    syncOwnDevice,
    syncQuarantinableAnchor,
  } from "$lib/sync/authority";

  let {
    buttonClass,
    context,
    mobileLayout = false,
  }: {
    buttonClass: string;
    context: SyncAuthorityContext;
    mobileLayout?: boolean;
  } = $props();

  type DeviceAction = "remove" | "quarantine" | "lift" | "promote";
  type AnchorAction = "takeAnchor" | "forceAnchor";
  type Confirmation = { kind: "remove" | "quarantine"; device: SyncAuthorityDevice } | { kind: AnchorAction };

  const { t } = getLocalization();
  const authorityState = getSyncAuthorityState();
  let busy = $state(false);
  let message = $state("");
  let failed = $state(false);
  let confirmation = $state<Confirmation | null>(null);

  const actionClass = $derived(mobileLayout ? `${buttonClass} min-h-12` : buttonClass);
  const own = $derived(syncOwnDevice(context.authority));
  const takeAnchor = $derived(canTakeSyncAnchor(context));
  const forceAnchor = $derived(canForceSyncAnchor(context));
  const lostAnchor = $derived(syncQuarantinableAnchor(context.authority));
  const lostAnchorName = $derived(lostAnchor ? syncAuthorityDeviceName(t, lostAnchor) : "");
  let quarantineAnchor = $state(false);
  const forceMessage = $derived(
    quarantineAnchor && lostAnchor
      ? `${t("sync.authority.forceAnchorMessage")} ${t("sync.authority.forceAnchorQuarantineNote", lostAnchorName)}`
      : t("sync.authority.forceAnchorMessage"),
  );

  function failureLabel(action: DeviceAction | AnchorAction): string {
    switch (action) {
      case "remove": return t("sync.authority.removeFailed");
      case "quarantine": return t("sync.authority.quarantineFailed");
      case "lift": return t("sync.authority.liftFailed");
      case "promote": return t("sync.authority.promoteFailed");
      case "takeAnchor": return t("sync.authority.takeAnchorFailed");
      case "forceAnchor": return t("sync.authority.forceAnchorFailed");
    }
  }

  function successLabel(action: DeviceAction | AnchorAction): string {
    if (action === "takeAnchor" || action === "forceAnchor") return t("sync.authority.takeAnchorDone");
    return action === "remove" || action === "promote" ? t("sync.authority.proposed") : "";
  }

  async function run(action: DeviceAction | AnchorAction, deviceId: string | null): Promise<void> {
    if (busy) return;
    busy = true;
    message = "";
    failed = false;
    try {
      if (action === "takeAnchor") await authorityState.takeAnchor();
      else if (action === "forceAnchor") {
        await authorityState.forceAnchor({ kitCode: null, quarantineAnchor: quarantineAnchor && lostAnchor !== null });
        quarantineAnchor = false;
      } else if (deviceId === null) throw new Error("A device action needs a device");
      else if (action === "remove") await authorityState.removeDevice(deviceId);
      else if (action === "quarantine") await authorityState.quarantineDevice(deviceId);
      else if (action === "lift") await authorityState.liftQuarantine(deviceId);
      else await authorityState.promoteDevice(deviceId);
      message = successLabel(action);
    } catch (cause: unknown) {
      console.warn("sync device action failed", cause);
      failed = true;
      message = failureLabel(action);
    } finally {
      busy = false;
    }
  }

  function confirm(): void {
    const current = confirmation;
    confirmation = null;
    if (!current) return;
    if ("device" in current) void run(current.kind, current.device.device.deviceId);
    else void run(current.kind, null);
  }
</script>

<div class="flex min-w-0 flex-col gap-2">
  <div class="px-1">
    <div class="text-[0.866667rem] text-foreground">{t("sync.authority.heading")}</div>
    <div class="mt-0.5 text-[0.8rem] text-muted-foreground">{t("sync.authority.description")}</div>
  </div>
  {#if own && own.removedRevision !== null}
    <Notice tone="warning" role="status" copy={false} class="px-1 text-[0.8rem] leading-5" message={t("sync.authority.ownRemoved")} />
  {/if}
  {#if context.authority.frozen}
    <Notice tone="warning" role="status" copy={false} class="px-1 text-[0.8rem] leading-5" message={t("sync.authority.frozen")} />
  {/if}
  {#each context.authority.devices as entry (entry.device.deviceId)}
    {@const actions = syncDeviceActions(context, entry)}
    <div class="flex min-w-0 flex-wrap items-center gap-x-3 gap-y-2 px-1">
      <div class="min-w-0 flex-1 basis-48">
        <div class="truncate text-[0.866667rem] font-medium text-foreground">{syncAuthorityDeviceName(t, entry)}</div>
        <div class="mt-0.5 flex flex-wrap gap-1 text-[0.75rem] text-muted-foreground">
          <span>{entry.kind === "desktop" ? t("sync.authority.kind.desktop") : t("sync.authority.kind.mobile")}</span>
          {#each syncDeviceBadges(entry) as badge (badge)}
            <span class="rounded-sm bg-muted px-1.5">{syncDeviceBadgeLabel(t, badge)}</span>
          {/each}
        </div>
      </div>
      {#if actions.remove || actions.quarantine || actions.lift || actions.promote}
        <div class="flex shrink-0 flex-wrap gap-2">
          {#if actions.promote}
            <button type="button" class={actionClass} disabled={busy} onclick={() => void run("promote", entry.device.deviceId)}>
              {t("sync.authority.promote")}
            </button>
          {/if}
          {#if actions.lift}
            <button type="button" class={actionClass} disabled={busy} onclick={() => void run("lift", entry.device.deviceId)}>
              {t("sync.authority.lift")}
            </button>
          {/if}
          {#if actions.quarantine}
            <button type="button" class={actionClass} disabled={busy} onclick={() => { confirmation = { kind: "quarantine", device: entry }; }}>
              {t("sync.authority.quarantine")}
            </button>
          {/if}
          {#if actions.remove}
            <button type="button" class={actionClass} disabled={busy} onclick={() => { confirmation = { kind: "remove", device: entry }; }}>
              {t("sync.authority.remove")}
            </button>
          {/if}
        </div>
      {/if}
    </div>
  {/each}
  {#if !mobileLayout && !context.authority.ownIsAdmin}
    <p class="px-1 text-[0.8rem] leading-5 text-muted-foreground">{t("sync.authority.notAdmin")}</p>
  {/if}
  {#if takeAnchor}
    <div class="flex flex-wrap gap-2">
      <button type="button" class={actionClass} disabled={busy} onclick={() => { confirmation = { kind: "takeAnchor" }; }}>
        {t("sync.authority.takeAnchor")}
      </button>
    </div>
  {/if}
  {#if forceAnchor}
    <div class="flex min-w-0 flex-col gap-2">
      {#if lostAnchor}
        <label class="flex min-w-0 items-start gap-2 px-1 text-[0.8rem] leading-5 text-foreground">
          <Checkbox class="mt-0.5" bind:checked={quarantineAnchor} disabled={busy} />
          <span class="min-w-0">{t("sync.authority.forceAnchorQuarantine", lostAnchorName)}</span>
        </label>
      {/if}
      <div class="flex flex-wrap gap-2">
        <button type="button" class={actionClass} disabled={busy} onclick={() => { confirmation = { kind: "forceAnchor" }; }}>
          {t("sync.authority.forceAnchor")}
        </button>
      </div>
    </div>
  {/if}
  {#if message}
    {#if failed}
      <Notice class="px-1 text-[0.8rem] leading-5" {message} copy={false} />
    {:else}
      <p role="status" class="px-1 text-[0.8rem] leading-5 text-muted-foreground">{message}</p>
    {/if}
  {/if}
</div>

{#if confirmation?.kind === "remove"}
  <ConfirmDialog
    title={t("sync.authority.removeTitle")}
    message={t("sync.authority.removeMessage", syncAuthorityDeviceName(t, confirmation.device))}
    confirmLabel={t("sync.authority.remove")}
    cancelLabel={t("common.cancel")}
    onConfirm={confirm}
    onCancel={() => { confirmation = null; }}
  />
{:else if confirmation?.kind === "quarantine"}
  <ConfirmDialog
    title={t("sync.authority.quarantineTitle")}
    message={context.authority.ownIsAdmin
      ? t("sync.authority.quarantineAdminMessage", syncAuthorityDeviceName(t, confirmation.device))
      : t("sync.authority.quarantineMessage", syncAuthorityDeviceName(t, confirmation.device))}
    confirmLabel={t("sync.authority.quarantine")}
    cancelLabel={t("common.cancel")}
    onConfirm={confirm}
    onCancel={() => { confirmation = null; }}
  />
{:else if confirmation?.kind === "takeAnchor"}
  <ConfirmDialog
    title={t("sync.authority.takeAnchorTitle")}
    message={t("sync.authority.takeAnchorMessage")}
    confirmLabel={t("sync.authority.takeAnchor")}
    cancelLabel={t("common.cancel")}
    danger={false}
    onConfirm={confirm}
    onCancel={() => { confirmation = null; }}
  />
{:else if confirmation?.kind === "forceAnchor"}
  <ConfirmDialog
    title={t("sync.authority.forceAnchorTitle")}
    message={forceMessage}
    confirmLabel={t("sync.authority.forceAnchor")}
    cancelLabel={t("common.cancel")}
    onConfirm={confirm}
    onCancel={() => { confirmation = null; }}
  />
{/if}
