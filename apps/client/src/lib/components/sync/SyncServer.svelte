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
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { syncServerErrorLabel, syncServerLines } from "$lib/sync/server";

  let {
    buttonClass,
    canManage,
  }: {
    buttonClass: string;
    /** Whether this live admin desktop may add or remove the server. */
    canManage: boolean;
  } = $props();

  /** The section of the distribution guide that covers running the self-hosted relay. */
  const RELAY_GUIDE_URL = "https://github.com/opengrimoire/ganbaru/blob/main/docs/operations/release/distribution.md#sync-relay";
  /** The guide that covers running an always-on anchor server. */
  const ANCHOR_GUIDE_URL = "https://github.com/opengrimoire/ganbaru/blob/main/docs/platforms/server/README.md";
  /** The text every anchor server setup code starts with. */
  const ANCHOR_CODE_PREFIX = "ganbaru-anchor:";

  const { t } = getLocalization();
  let server = $state<SyncServerView | null>(null);
  let loading = $state(true);
  let loadFailed = $state(false);
  let busy = $state(false);
  let code = $state("");
  let acknowledged = $state(false);
  let takeAnchor = $state(true);
  /** Whether an anchor server is being added and the form stays up to repeat it with the same code. */
  let resuming = $state(false);
  let message = $state("");
  let failed = $state(false);
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

  function showError(cause: unknown): void {
    console.warn("sync server change failed", cause);
    failed = true;
    message = cause instanceof SyncServerError ? syncServerErrorLabel(t, cause) : t("sync.server.errors.failed");
  }

  async function add(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    const entered = code.trim();
    if (busy || entered.length === 0 || !acknowledged) return;
    busy = true;
    message = "";
    failed = false;
    try {
      const step = await addSyncServer(entered, acknowledged, isAnchorCode && takeAnchor);
      resuming = step === "waitingForAnchor";
      if (!resuming) {
        code = "";
        acknowledged = false;
      }
      message = addedMessage(step);
      await refresh();
    } catch (cause: unknown) {
      showError(cause);
    } finally {
      busy = false;
    }
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
    if (busy) return;
    busy = true;
    message = "";
    failed = false;
    try {
      const removal = await removeSyncServer();
      message = removedMessage(removal);
      await refresh();
    } catch (cause: unknown) {
      showError(cause);
    } finally {
      busy = false;
    }
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
        <input
          id="sync-server-code"
          class="field h-9 w-full px-3 font-mono text-sm"
          dir="ltr"
          autocomplete="off"
          autocapitalize="off"
          spellcheck="false"
          placeholder={t("sync.server.codePlaceholder")}
          bind:value={code}
          disabled={busy}
        />
        <label class="flex min-w-0 items-start gap-2 text-[0.8rem] leading-5 text-foreground">
          <Checkbox class="mt-0.5" bind:checked={acknowledged} disabled={busy} />
          <span class="min-w-0">{isAnchorCode ? t("sync.server.unreviewedAnchor") : t("sync.server.unreviewed")}</span>
        </label>
        {#if isAnchorCode}
          <p class="text-[0.8rem] leading-5 text-foreground">{t("sync.server.anchorReadable")}</p>
          <label class="flex min-w-0 items-start gap-2 text-[0.8rem] leading-5 text-foreground">
            <Checkbox class="mt-0.5" bind:checked={takeAnchor} disabled={busy} />
            <span class="min-w-0">{t("sync.server.takeAnchor")}</span>
          </label>
        {/if}
        <div class="flex flex-wrap gap-2">
          <button type="submit" class={buttonClass} disabled={busy || !acknowledged || code.trim().length === 0}>
            {busy ? t("sync.server.adding") : t("sync.server.add")}
          </button>
        </div>
      </form>
    {:else}
      <p class="px-1 text-[0.8rem] leading-5 text-muted-foreground">{t("sync.server.adminOnly")}</p>
    {/if}
  {/if}

  <div class="flex flex-wrap gap-x-4 gap-y-1 px-1">
    <button type="button" class="text-[0.8rem] text-muted-foreground underline underline-offset-2" onclick={() => void openGuide(RELAY_GUIDE_URL)}>
      {t("sync.server.guide")}
    </button>
    <button type="button" class="text-[0.8rem] text-muted-foreground underline underline-offset-2" onclick={() => void openGuide(ANCHOR_GUIDE_URL)}>
      {t("sync.server.anchorGuide")}
    </button>
  </div>

  {#if message}
    {#if failed}
      <Notice class="px-1 text-[0.8rem] leading-5" {message} copy={false} />
    {:else}
      <p role="status" class="px-1 text-[0.8rem] leading-5 text-muted-foreground">{message}</p>
    {/if}
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
