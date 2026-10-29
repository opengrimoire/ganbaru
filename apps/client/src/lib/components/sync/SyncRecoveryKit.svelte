<script lang="ts">
  import { saveSyncRecoveryKitFile, type SyncAuthorityDevice, type SyncPendingRecoveryKit } from "$lib/api/sync";
  import Checkbox from "$lib/components/ui/Checkbox.svelte";
  import ConfirmDialog from "$lib/components/ui/ConfirmDialog.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getSyncAuthorityState } from "$lib/stores/sync-authority.svelte";
  import {
    syncAuthorityDeviceName,
    type SyncKitUse,
    syncKitCodeLines,
    syncKitFileName,
    syncKitFileText,
  } from "$lib/sync/authority";

  let {
    buttonClass,
    canManage,
    canInstallKey,
    use,
    anchor,
    pendingKit,
  }: {
    buttonClass: string;
    /** Whether this admin desktop shows and saves a waiting kit or recovery key. */
    canManage: boolean;
    /** Whether this anchor desktop may install a new recovery key. */
    canInstallKey: boolean;
    /** How this desktop may use a kit to become an admin, or null when it may not. */
    use: SyncKitUse | null;
    /** The anchor a forced handover may also quarantine, when there is one. */
    anchor: SyncAuthorityDevice | null;
    pendingKit: SyncPendingRecoveryKit | null;
  } = $props();

  const localization = getLocalization();
  const { t } = localization;
  const authorityState = getSyncAuthorityState();
  let busy = $state(false);
  let message = $state("");
  let failed = $state(false);
  let confirmation = $state<"saved" | "regenerate" | "force" | null>(null);
  let code = $state("");
  let quarantineAnchor = $state(false);

  const anchorName = $derived(anchor ? syncAuthorityDeviceName(t, anchor) : "");
  const forceMessage = $derived(
    quarantineAnchor && anchor
      ? `${t("sync.authority.forceAnchorMessage")} ${t("sync.authority.forceAnchorQuarantineNote", anchorName)}`
      : t("sync.authority.forceAnchorMessage"),
  );

  const codeLines = $derived(pendingKit ? syncKitCodeLines(pendingKit.code) : []);

  async function run(action: () => Promise<unknown>, success: string, failure: string): Promise<void> {
    if (busy) return;
    busy = true;
    message = "";
    failed = false;
    try {
      await action();
      message = success;
    } catch (cause: unknown) {
      console.warn("recovery kit action failed", cause);
      failed = true;
      message = failure;
    } finally {
      busy = false;
    }
  }

  function saveFile(kit: SyncPendingRecoveryKit): Promise<void> {
    return run(
      () => saveSyncRecoveryKitFile(syncKitFileName(t), syncKitFileText(t, localization.locale, kit.code, Date.now())),
      "",
      t("sync.kit.saveFailed"),
    );
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

  /** Forces the handover with the entered kit, quarantining the old anchor when asked. */
  async function forceWithKit(): Promise<void> {
    const entered = code.trim();
    if (!entered) return;
    const request = { kitCode: entered, quarantineAnchor: quarantineAnchor && anchor !== null };
    await run(() => authorityState.forceAnchor(request), t("sync.kit.forceDone"), t("sync.kit.useFailed"));
    if (!failed) {
      code = "";
      quarantineAnchor = false;
    }
  }

  function confirm(): void {
    const current = confirmation;
    confirmation = null;
    if (current === "saved") void run(authorityState.confirmKitSaved, "", t("sync.kit.confirmFailed"));
    else if (current === "regenerate") void run(authorityState.regenerateKit, "", t("sync.kit.regenerateFailed"));
    else if (current === "force") void forceWithKit();
  }
</script>

<div class="flex min-w-0 flex-col gap-2">
  <div class="px-1">
    <div class="text-[0.866667rem] text-foreground">{t("sync.kit.heading")}</div>
    <div class="mt-0.5 text-[0.8rem] text-muted-foreground">{t("sync.kit.description")}</div>
  </div>
  {#if canManage && pendingKit}
    <Notice tone="warning" role="status" copy={false} class="px-1 text-[0.8rem] leading-5" message={t("sync.kit.pending")} />
    <div class="px-1" aria-label={t("sync.kit.code")}>
      {#each codeLines as line, index (index)}
        <p class="font-mono text-[0.933333rem] font-semibold tracking-wider text-foreground select-all" dir="ltr">{line}</p>
      {/each}
    </div>
    {#if !pendingKit.active}
      <p class="px-1 text-[0.8rem] leading-5 text-muted-foreground">{t("sync.kit.inactive")}</p>
    {/if}
    <div class="flex flex-wrap gap-2">
      <button type="button" class={buttonClass} disabled={busy} onclick={() => void saveFile(pendingKit)}>
        {t("sync.kit.saveFile")}
      </button>
      <button type="button" class={buttonClass} disabled={busy} onclick={() => { confirmation = "saved"; }}>
        {t("sync.kit.confirmSaved")}
      </button>
    </div>
  {:else if canInstallKey}
    <div class="flex flex-wrap gap-2">
      <button type="button" class={buttonClass} disabled={busy} onclick={() => { confirmation = "regenerate"; }}>
        {t("sync.kit.regenerate")}
      </button>
    </div>
  {/if}
  {#if use}
    <form class="flex min-w-0 flex-col gap-2 px-1" onsubmit={(event) => void useKit(event)}>
      <label class="text-[0.8rem] text-muted-foreground" for="sync-recovery-kit-code">
        {use === "force" ? t("sync.kit.forceDescription") : t("sync.kit.useDescription")}
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

{#if confirmation === "saved"}
  <ConfirmDialog
    title={t("sync.kit.confirmSavedTitle")}
    message={t("sync.kit.confirmSavedMessage")}
    confirmLabel={t("sync.kit.confirmSaved")}
    cancelLabel={t("common.cancel")}
    onConfirm={confirm}
    onCancel={() => { confirmation = null; }}
  />
{:else if confirmation === "force"}
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
{/if}
