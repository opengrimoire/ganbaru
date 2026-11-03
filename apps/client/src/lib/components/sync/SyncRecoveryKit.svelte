<script lang="ts">
  import type { SyncAuthorityDevice, SyncContests, SyncPendingRecoveryKit } from "$lib/api/sync";
  import Checkbox from "$lib/components/ui/Checkbox.svelte";
  import ConfirmDialog from "$lib/components/ui/ConfirmDialog.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import Select from "$lib/components/ui/Select.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getSyncAuthorityState } from "$lib/stores/sync-authority.svelte";
  import { syncAuthorityDeviceName, type SyncKitUse } from "$lib/sync/authority";
  import SyncRecoveryKitSheet from "./SyncRecoveryKitSheet.svelte";
  import { syncContestDeviceName, syncKitPointLabel, syncKitPoints, syncKitReviewDevices } from "$lib/sync/contests";

  let {
    buttonClass,
    canManage,
    canInstallKey,
    canRotate,
    use,
    anchor,
    pendingKit,
    contests = null,
  }: {
    buttonClass: string;
    /**
     * Whether this admin desktop shows and saves a waiting kit or recovery key. Any other
     * desktop explains that only an admin computer can.
     */
    canManage: boolean;
    /** Whether this anchor desktop may install a new recovery key. */
    canInstallKey: boolean;
    /**
     * Whether this desktop may replace the printed kit with its code, admin or not. It then
     * shows and saves the new kit too.
     */
    canRotate: boolean;
    /** How this desktop may use a kit to become an admin, or null when it may not. */
    use: SyncKitUse | null;
    /** The anchor a forced handover may also quarantine, when there is one. */
    anchor: SyncAuthorityDevice | null;
    pendingKit: SyncPendingRecoveryKit | null;
    /** Open contests, whose kit points and device review a forced handover offers. */
    contests?: SyncContests | null;
  } = $props();

  const { t } = getLocalization();
  const authorityState = getSyncAuthorityState();
  let busy = $state(false);
  let message = $state("");
  let failed = $state(false);
  let confirmation = $state<"regenerate" | "force" | "rotate" | null>(null);
  let code = $state("");
  let rotating = $state(false);
  let rotateCode = $state("");
  let quarantineAnchor = $state(false);
  /** The user's marks in the device review, by device id, over each device's preselection. */
  let reviewMarks = $state<Record<string, boolean>>({});
  let pointHash = $state("");

  const contested = $derived(use === "force" && syncKitPoints(contests).length > 0);
  const kitPoints = $derived(contested ? syncKitPoints(contests) : []);
  const reviewDevices = $derived(contested ? syncKitReviewDevices(contests) : []);
  const pointOptions = $derived(
    kitPoints.map((point) => ({ value: point.entry.hash, label: syncKitPointLabel(t, point) })),
  );
  const selectedPoint = $derived(kitPoints.find((point) => point.entry.hash === pointHash) ?? kitPoints[0] ?? null);

  function reviewMarked(deviceId: string, preselected: boolean): boolean {
    return reviewMarks[deviceId] ?? preselected;
  }

  const anchorName = $derived(anchor ? syncAuthorityDeviceName(t, anchor) : "");
  const forceMessage = $derived(
    quarantineAnchor && anchor
      ? `${t("sync.authority.forceAnchorMessage")} ${t("sync.authority.forceAnchorQuarantineNote", anchorName)} ${t("sync.kit.forceRecoveryKeyNote")}`
      : `${t("sync.authority.forceAnchorMessage")} ${t("sync.kit.forceRecoveryKeyNote")}`,
  );

  /**
   * Runs a kit action and shows `success`, or the message it derives from the action's result,
   * or `failure` when the action throws.
   */
  async function run<T>(action: () => Promise<T>, success: string | ((result: T) => string), failure: string): Promise<void> {
    if (busy) return;
    busy = true;
    message = "";
    failed = false;
    try {
      const result = await action();
      message = typeof success === "function" ? success(result) : success;
    } catch (cause: unknown) {
      console.warn("recovery kit action failed", cause);
      failed = true;
      message = failure;
    } finally {
      busy = false;
    }
  }

  async function useKit(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    const entered = code.trim();
    if (!entered) return;
    if (use === "force") {
      confirmation = "force";
      return;
    }
    await run(() => authorityState.useKit(entered), t("sync.kit.useSent"), t("sync.kit.useFailed"));
    if (!failed) code = "";
  }

  /**
   * Forces the handover with the entered kit, quarantining the old anchor when asked. In a
   * contest it continues from the chosen kit point, null for the default one, and removes the
   * devices the review marked.
   */
  async function forceWithKit(): Promise<void> {
    const entered = code.trim();
    if (!entered) return;
    const point = selectedPoint && selectedPoint !== kitPoints[0] ? selectedPoint.entry : null;
    const removeDevices = reviewDevices
      .filter((entry) => reviewMarked(entry.device.deviceId, entry.preselected))
      .map((entry) => entry.device.deviceId);
    const request = { kitCode: entered, point, removeDevices, quarantineAnchor: quarantineAnchor && anchor !== null };
    await run(
      () => authorityState.forceAnchor(request),
      (outcome) => (outcome.incomplete > 0 ? t("sync.kit.forcePartial") : t("sync.kit.forceDone")),
      t("sync.kit.useFailed"),
    );
    if (!failed) {
      code = "";
      quarantineAnchor = false;
      reviewMarks = {};
      pointHash = "";
    }
  }

  function askRotate(event: SubmitEvent): void {
    event.preventDefault();
    if (rotateCode.trim()) confirmation = "rotate";
  }

  /** Replaces the printed kit with the entered code; the new kit then waits to be saved. */
  async function rotateKit(): Promise<void> {
    const entered = rotateCode.trim();
    if (!entered) return;
    await run(() => authorityState.rotateKit(entered), "", t("sync.kit.rotateFailed"));
    if (!failed) {
      rotateCode = "";
      rotating = false;
    }
  }

  function confirm(): void {
    const current = confirmation;
    confirmation = null;
    if (current === "regenerate") void run(authorityState.regenerateKit, "", t("sync.kit.regenerateFailed"));
    else if (current === "force") void forceWithKit();
    else if (current === "rotate") void rotateKit();
  }
</script>

<div class="flex min-w-0 flex-col gap-2">
  <div class="px-1">
    <div class="text-[0.866667rem] text-foreground">{t("sync.kit.heading")}</div>
    <div class="mt-0.5 text-[0.8rem] text-muted-foreground">{t("sync.kit.description")}</div>
  </div>
  {#if pendingKit && (canManage || canRotate)}
    <SyncRecoveryKitSheet kit={pendingKit} {buttonClass} onConfirm={authorityState.confirmKitSaved} />
  {:else}
    {#if !canManage && !canRotate}
      <p class="px-1 text-[0.8rem] leading-5 text-muted-foreground">{t("sync.kit.adminOnly")}</p>
    {/if}
    {#if canRotate && rotating}
      <form class="flex min-w-0 flex-col gap-2 px-1" onsubmit={askRotate}>
        <label class="text-[0.8rem] text-muted-foreground" for="sync-recovery-kit-rotate-code">
          {t("sync.kit.rotateDescription")}
        </label>
        <input
          id="sync-recovery-kit-rotate-code"
          class="field h-9 w-full px-3 font-mono text-sm"
          dir="ltr"
          autocomplete="off"
          autocapitalize="characters"
          spellcheck="false"
          placeholder={t("sync.kit.code")}
          bind:value={rotateCode}
          disabled={busy}
        />
        <div class="flex flex-wrap gap-2">
          <button type="submit" class={buttonClass} disabled={busy || rotateCode.trim().length === 0}>
            {t("sync.kit.rotateSubmit")}
          </button>
          <button type="button" class={buttonClass} disabled={busy} onclick={() => { rotating = false; rotateCode = ""; }}>
            {t("common.cancel")}
          </button>
        </div>
      </form>
    {:else if canInstallKey || canRotate}
      <div class="flex flex-wrap gap-2">
        {#if canInstallKey}
          <button type="button" class={buttonClass} disabled={busy} onclick={() => { confirmation = "regenerate"; }}>
            {t("sync.kit.regenerate")}
          </button>
        {/if}
        {#if canRotate}
          <button type="button" class={buttonClass} disabled={busy} onclick={() => { rotating = true; }}>
            {t("sync.kit.rotateAction")}
          </button>
        {/if}
      </div>
    {/if}
  {/if}
  {#if use}
    <form class="flex min-w-0 flex-col gap-2 px-1" onsubmit={(event) => void useKit(event)}>
      <label class="text-[0.8rem] text-muted-foreground" for="sync-recovery-kit-code">
        {contested ? t("sync.kit.contestDescription") : use === "force" ? t("sync.kit.forceDescription") : t("sync.kit.useDescription")}
      </label>
      <input
        id="sync-recovery-kit-code"
        class="field h-9 w-full px-3 font-mono text-sm"
        dir="ltr"
        autocomplete="off"
        autocapitalize="characters"
        spellcheck="false"
        placeholder={t("sync.kit.code")}
        bind:value={code}
        disabled={busy}
      />
      {#if use === "force" && anchor}
        <label class="flex min-w-0 items-start gap-2 text-[0.8rem] leading-5 text-foreground">
          <Checkbox class="mt-0.5" bind:checked={quarantineAnchor} disabled={busy} />
          <span class="min-w-0">{t("sync.authority.forceAnchorQuarantine", anchorName)}</span>
        </label>
      {/if}
      {#if pointOptions.length > 1 && selectedPoint}
        <Select
          label={t("sync.kit.pointLabel")}
          value={selectedPoint.entry.hash}
          options={pointOptions}
          disabled={busy}
          onChange={(value) => { pointHash = value; }}
        />
      {/if}
      {#if reviewDevices.length > 0}
        <div class="flex min-w-0 flex-col gap-1.5">
          <div class="text-[0.8rem] text-foreground">{t("sync.kit.reviewHeading")}</div>
          <div class="text-[0.75rem] text-muted-foreground">{t("sync.kit.reviewDescription")}</div>
          {#each reviewDevices as entry (entry.device.deviceId)}
            <label class="flex min-w-0 items-start gap-2 text-[0.8rem] leading-5 text-foreground">
              <Checkbox
                class="mt-0.5"
                checked={reviewMarked(entry.device.deviceId, entry.preselected)}
                disabled={busy}
                onChange={(checked: boolean) => { reviewMarks[entry.device.deviceId] = checked; }}
              />
              <span class="flex min-w-0 flex-col">
                <span class="truncate">{syncContestDeviceName(t, entry.device)}</span>
                <span class="text-[0.75rem] text-muted-foreground">
                  {entry.reason === "flooder"
                    ? t("sync.kit.reviewFlooder")
                    : entry.removedBy
                      ? t("sync.kit.reviewRemovedBy", syncContestDeviceName(t, entry.removedBy))
                      : t("sync.kit.reviewRemovedByKey")}
                </span>
              </span>
            </label>
          {/each}
        </div>
      {/if}
      <div class="flex flex-wrap gap-2">
        <button type="submit" class={buttonClass} disabled={busy || code.trim().length === 0}>
          {use === "force" ? t("sync.kit.forceSubmit") : t("sync.kit.useSubmit")}
        </button>
      </div>
    </form>
  {/if}
  {#if message}
    {#if failed}
      <Notice class="px-1 text-[0.8rem] leading-5" {message} copy={false} />
    {:else}
      <p role="status" class="px-1 text-[0.8rem] leading-5 text-muted-foreground">{message}</p>
    {/if}
  {/if}
</div>

{#if confirmation === "force"}
  <ConfirmDialog
    title={t("sync.authority.forceAnchorTitle")}
    message={forceMessage}
    confirmLabel={t("sync.kit.forceSubmit")}
    cancelLabel={t("common.cancel")}
    onConfirm={confirm}
    onCancel={() => { confirmation = null; }}
  />
{:else if confirmation === "regenerate"}
  <ConfirmDialog
    title={t("sync.kit.regenerateTitle")}
    message={t("sync.kit.regenerateMessage")}
    confirmLabel={t("sync.kit.regenerate")}
    cancelLabel={t("common.cancel")}
    onConfirm={confirm}
    onCancel={() => { confirmation = null; }}
  />
{:else if confirmation === "rotate"}
  <ConfirmDialog
    title={t("sync.kit.rotateTitle")}
    message={t("sync.kit.rotateMessage")}
    confirmLabel={t("sync.kit.rotateSubmit")}
    cancelLabel={t("common.cancel")}
    onConfirm={confirm}
    onCancel={() => { confirmation = null; }}
  />
{/if}
