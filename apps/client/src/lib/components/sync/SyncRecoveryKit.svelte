<script lang="ts">
  import { saveSyncRecoveryKitFile, type SyncPendingRecoveryKit } from "$lib/api/sync";
  import ConfirmDialog from "$lib/components/ui/ConfirmDialog.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getSyncAuthorityState } from "$lib/stores/sync-authority.svelte";
  import { syncKitCodeLines, syncKitFileName, syncKitFileText } from "$lib/sync/authority";

  let {
    buttonClass,
    canManage,
    canUse,
    pendingKit,
  }: {
    buttonClass: string;
    /** Whether this admin desktop shows, saves, and replaces the kit. */
    canManage: boolean;
    /** Whether this desktop may use a kit to become an admin. */
    canUse: boolean;
    pendingKit: SyncPendingRecoveryKit | null;
  } = $props();

  const localization = getLocalization();
  const { t } = localization;
  const authorityState = getSyncAuthorityState();
  let busy = $state(false);
  let message = $state("");
  let failed = $state(false);
  let confirmation = $state<"saved" | "regenerate" | null>(null);
  let code = $state("");

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
    await run(() => authorityState.useKit(entered), t("sync.kit.useSent"), t("sync.kit.useFailed"));
    if (!failed) code = "";
  }

  function confirm(): void {
    const current = confirmation;
    confirmation = null;
    if (current === "saved") void run(authorityState.confirmKitSaved, "", t("sync.kit.confirmFailed"));
    else if (current === "regenerate") void run(authorityState.regenerateKit, "", t("sync.kit.regenerateFailed"));
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
  {:else if canManage}
    <div class="flex flex-wrap gap-2">
      <button type="button" class={buttonClass} disabled={busy} onclick={() => { confirmation = "regenerate"; }}>
        {t("sync.kit.regenerate")}
      </button>
    </div>
  {/if}
  {#if canUse}
    <form class="flex min-w-0 flex-col gap-2 px-1" onsubmit={(event) => void useKit(event)}>
      <label class="text-[0.8rem] text-muted-foreground" for="sync-recovery-kit-code">{t("sync.kit.useDescription")}</label>
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
      <div class="flex flex-wrap gap-2">
        <button type="submit" class={buttonClass} disabled={busy || code.trim().length === 0}>{t("sync.kit.useSubmit")}</button>
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
