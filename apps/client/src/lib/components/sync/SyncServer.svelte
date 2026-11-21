<script lang="ts">
  import { openUrl } from "@tauri-apps/plugin-opener";
  import {
    addSyncServer,
    getSyncServerStatus,
    removeSyncServer,
    SyncServerError,
    type SyncServerAddStep,
    type SyncServerRemoval,
    type SyncServerView,
  } from "$lib/api/sync";
  import Checkbox from "$lib/components/ui/Checkbox.svelte";
  import ConfirmDialog from "$lib/components/ui/ConfirmDialog.svelte";
  import MaskedCodeInput from "$lib/components/ui/MaskedCodeInput.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { SERVER_GUIDE_URL, syncServerErrorLabel, syncServerLines } from "$lib/sync/server";
  import { IDLE_SYNC_ACTIONS, runSyncAction, type SyncActionAccess, type SyncActionState } from "./sync-action-state";

  let {
    buttonClass,
    canManage,
    onAdded,
  }: {
    buttonClass: string;
    /** Whether this live admin desktop may add or remove the server. */
    canManage: boolean;
    /** Runs after a server was added, so a desktop that synced with nothing can show sync. */
    onAdded?: () => void;
  } = $props();

  /** The self-hosted server guide, which also explains syncing devices in different places. */
  /** The text every anchor server setup code starts with. */
  const ANCHOR_CODE_PREFIX = "ganbaru-anchor:";

  const { t } = getLocalization();
  let server = $state<SyncServerView | null>(null);
  let loading = $state(true);
  let loadFailed = $state(false);
  let actions = $state<SyncActionState<"server">>(IDLE_SYNC_ACTIONS);
  const actionAccess: SyncActionAccess<"server"> = { read: () => actions, write: (next) => { actions = next; } };
  const busy = $derived(actions.running.length > 0);
  let code = $state("");
  let takeAnchor = $state(true);
  /** Whether an anchor server is being added and the form stays up to repeat it with the same code. */
  let resuming = $state(false);
  let removeConfirming = $state(false);

  const lines = $derived(server ? syncServerLines(t, server) : []);
  const isAnchorCode = $derived(code.trim().startsWith(ANCHOR_CODE_PREFIX));

  /** Loads the server the vault names and probes a relay; a probe failure is part of the view. */
  async function refresh(): Promise<void> {
    loading = true;
    try {
      server = await getSyncServerStatus();
      loadFailed = false;
    } catch (cause: unknown) {
      console.warn("sync server status could not be loaded", cause);
      loadFailed = true;
    } finally {
      loading = false;
    }
  }

  /** The message for a failed server change, which it also logs. */
  function errorMessage(cause: unknown): string {
    console.warn("sync server change failed", cause);
    return cause instanceof SyncServerError ? syncServerErrorLabel(t, cause) : t("sync.server.errors.failed");
  }

  async function add(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    const entered = code.trim();
    if (entered.length === 0) return;
    await runSyncAction(actionAccess, "server", async () => {
      const step = await addSyncServer(entered, isAnchorCode && takeAnchor);
      resuming = step === "waitingForAnchor";
      if (!resuming) {
        code = "";
      }
      await refresh();
      onAdded?.();
      return step;
    }, { success: addedMessage, failure: errorMessage, exclusive: true });
  }

  function addedMessage(step: SyncServerAddStep): string {
    if (step === "active") return t("sync.server.anchorActive");
    if (step === "waitingForAnchor") return t("sync.server.anchorWaiting");
    return t("sync.server.added");
  }

  function removedMessage(removal: SyncServerRemoval): string {
    if (removal.relayCleared) return t("sync.server.removed");
    return removal.problem === null ? t("sync.server.removedAnchor") : t("sync.server.removedRelayKept");
  }

  async function remove(): Promise<void> {
    removeConfirming = false;
    await runSyncAction(actionAccess, "server", async () => {
      const removal = await removeSyncServer();
      await refresh();
      return removal;
    }, { success: removedMessage, failure: errorMessage, exclusive: true });
  }

  async function openGuide(url: string): Promise<void> {
    try {
      await openUrl(url);
    } catch (cause: unknown) {
      console.warn("server setup guide could not be opened", cause);
    }
  }

  $effect(() => {
    void refresh();
  });
</script>

<div class="flex min-w-0 flex-col gap-2">
  <div class="px-1">
    <div class="text-[0.866667rem] text-foreground">{t("sync.server.heading")}</div>
    <div class="mt-0.5 text-[0.8rem] text-muted-foreground">{t("sync.server.description")}</div>
  </div>

  {#if loadFailed && !server}
    <Notice class="px-1 text-[0.8rem] leading-5" message={t("sync.server.loadFailed")} copy={false} onRetry={refresh} />
  {:else if server && !resuming}
    <div class="flex min-w-0 flex-col gap-0.5 px-1 text-[0.8rem] leading-5" role="status">
      <div class="text-foreground">{server.mode === "relay" ? t("sync.server.relay") : t("sync.server.anchor")}</div>
      {#each lines as line (line)}
        <div class="min-w-0 break-all text-muted-foreground">{line}</div>
      {/each}
      {#if loading}
        <div class="text-muted-foreground">{t("sync.server.checking")}</div>
      {/if}
    </div>
    <div class="flex flex-wrap gap-2">
      <button type="button" class={buttonClass} disabled={loading || busy} onclick={() => void refresh()}>
        {t("sync.server.check")}
      </button>
      {#if canManage && server.canManage}
        <button type="button" class={buttonClass} disabled={busy} onclick={() => { removeConfirming = true; }}>
          {t("sync.server.remove")}
        </button>
      {/if}
    </div>
  {:else if !loading || resuming}
    {#if !resuming}
      <p class="px-1 text-[0.8rem] leading-5 text-muted-foreground">{t("sync.server.none")}</p>
    {/if}
    {#if canManage}
      <form class="flex min-w-0 flex-col gap-2 px-1" onsubmit={(event) => void add(event)}>
        <label class="text-[0.8rem] text-muted-foreground" for="sync-server-code">{t("sync.server.codeLabel")}</label>
        <MaskedCodeInput
          id="sync-server-code"
          class="field w-full px-3 font-mono text-sm"
          clearLabel={t("common.clearCode")}
          dir="ltr"
          autocomplete="off"
          autocapitalize="off"
          spellcheck="false"
          placeholder={t("sync.server.codePlaceholder")}
          bind:value={code}
          disabled={busy}
        />
        {#if isAnchorCode}
          <p class="text-[0.8rem] leading-5 text-foreground">{t("sync.server.anchorReadable")}</p>
          <label class="flex min-w-0 items-start gap-2 text-[0.8rem] leading-5 text-foreground">
            <Checkbox class="mt-0.5" bind:checked={takeAnchor} disabled={busy} />
            <span class="min-w-0">{t("sync.server.takeAnchor")}</span>
          </label>
        {/if}
        <div class="flex flex-wrap gap-2">
          <button type="submit" class={buttonClass} disabled={busy || code.trim().length === 0}>
            {busy ? t("sync.server.adding") : t("sync.server.add")}
          </button>
        </div>
      </form>
    {:else}
      <p class="px-1 text-[0.8rem] leading-5 text-muted-foreground">{t("sync.server.adminOnly")}</p>
    {/if}
  {/if}

  <div class="flex flex-wrap gap-x-4 gap-y-1 px-1">
    <button type="button" class="text-[0.8rem] text-muted-foreground underline underline-offset-2" onclick={() => void openGuide(`${SERVER_GUIDE_URL}#do-you-need-a-server`)}>
      {t("sync.server.placesGuide")}
    </button>
    <button type="button" class="text-[0.8rem] text-muted-foreground underline underline-offset-2" onclick={() => void openGuide(`${SERVER_GUIDE_URL}#anchor-or-relay`)}>
      {t("sync.server.guide")}
    </button>
  </div>

  {#if actions.feedback.kind === "failed"}
    <Notice class="px-1 text-[0.8rem] leading-5" message={actions.feedback.message} copy={false} />
  {:else if actions.feedback.kind === "done"}
    <p role="status" class="px-1 text-[0.8rem] leading-5 text-muted-foreground">{actions.feedback.message}</p>
  {/if}
</div>

{#if removeConfirming}
  <ConfirmDialog
    title={t("sync.server.removeTitle")}
    message={t("sync.server.removeMessage")}
    confirmLabel={t("sync.server.remove")}
    cancelLabel={t("common.cancel")}
    onConfirm={() => void remove()}
    onCancel={() => { removeConfirming = false; }}
  />
{/if}
