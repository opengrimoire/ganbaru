<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import FolderInput from "@lucide/svelte/icons/folder-input";
  import FolderOpen from "@lucide/svelte/icons/folder-open";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import VaultHandoffPanel from "$lib/components/vault/handoff/VaultHandoffPanel.svelte";
  import Loading from "$lib/components/ui/Loading.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import {
    formatVaultError,
    getCachedActiveVaultInfo,
    getActiveVaultInfo,
    pickOpenVault,
    pickCreateVault,
    revealActiveVault,
    type VaultInfo,
  } from "$lib/vault/state";

  const cachedVault = getCachedActiveVaultInfo();
  let activeVault = $state<VaultInfo | null>(cachedVault ?? null);
  let folderStateLoaded = $state(cachedVault !== undefined);
  let busy = $state<"load" | "reveal" | "change" | "import" | null>(
    cachedVault === undefined ? "load" : null,
  );
  let error = $state<string | null>(null);
  const { t } = getLocalization();

  const actionButtonClass =
    "inline-flex h-7 shrink-0 items-center justify-center gap-1.5 rounded-md border border-border px-2.5 text-[0.8rem] font-medium transition-colors focus:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:pointer-events-none disabled:opacity-55";
  const secondaryButtonClass = `${actionButtonClass} bg-card text-foreground hover:bg-accent dark:bg-transparent`;

  const currentFolderPath = $derived(activeVault?.path ?? t("settings.data.noActiveFolder"));

  async function loadVaultState(): Promise<void> {
    const showLoadingState = !folderStateLoaded;
    if (showLoadingState) busy = "load";
    error = null;
    try {
      activeVault = await getActiveVaultInfo();
    } catch (err) {
      error = formatVaultError(err, "startup", t);
    } finally {
      folderStateLoaded = true;
      if (showLoadingState) busy = null;
    }
  }

  /**
   * Reloads the active vault status for the error notice retry action.
   *
   * @returns A promise that rejects when the status still cannot be read.
   */
  async function retryVaultState(): Promise<void> {
    await loadVaultState();
    if (error) throw new Error(error);
  }

  function restartApp(): void {
    void invoke("restart_app");
  }

  async function revealVault(): Promise<void> {
    busy = "reveal";
    error = null;
    try {
      await revealActiveVault();
    } catch (err) {
      error = formatVaultError(err, "general", t);
    } finally {
      busy = null;
    }
  }

  async function chooseVault(mode: "change" | "import"): Promise<void> {
    busy = mode;
    error = null;
    try {
      const info = mode === "change" ? await pickCreateVault() : await pickOpenVault();
      if (info) restartApp();
    } catch (err) {
      error = formatVaultError(err, mode, t);
    } finally {
      busy = null;
    }
  }

  $effect(() => {
    void loadVaultState();
  });
</script>

<div class="flex flex-col gap-6">
  <VaultHandoffPanel platform="desktop" onActivated={restartApp} />

  <div class="h-px shrink-0 scale-y-50 bg-border" aria-hidden="true"></div>

  <section class="flex flex-col gap-4">
    <h2 class="px-1 text-[0.866667rem] font-semibold text-foreground">{t("settings.data.folderHeading")}</h2>

    <div class="flex flex-col gap-3">
      <div class="flex items-start justify-between gap-4 px-1 py-1 max-[520px]:flex-col max-[520px]:items-stretch max-[520px]:gap-2">
        <div class="min-w-0 flex-1">
          <div class="text-[0.866667rem] text-foreground">{t("settings.data.currentFolder")}</div>
          <div class="mt-0.5 wrap-break-word text-[0.8rem] leading-5 text-muted-foreground">
            {#if folderStateLoaded}
              {currentFolderPath}
            {:else}
              <Loading layout="inline" label={t("settings.data.loadingFolder")} />
            {/if}
          </div>
        </div>
        <button
          type="button"
          onclick={() => void revealVault()}
          disabled={busy !== null || !activeVault}
          class={secondaryButtonClass}
        >
          {#if busy === "reveal"}
            <LoaderCircle size={14} strokeWidth={2.1} class="shrink-0 animate-spin" />
          {:else}
            <FolderOpen size={14} strokeWidth={1.9} class="shrink-0" />
          {/if}
          <span>{t("settings.data.reveal")}</span>
        </button>
      </div>

      <div class="flex items-start justify-between gap-4 px-1 py-1 max-[520px]:flex-col max-[520px]:items-stretch max-[520px]:gap-2">
        <div class="min-w-0 flex-1">
          <div class="text-[0.866667rem] text-foreground">{t("settings.data.changeFolder")}</div>
          <div class="mt-0.5 text-[0.8rem] leading-5 text-muted-foreground">
            {t("settings.data.changeFolderDescription")}
          </div>
        </div>
        <button
          type="button"
          onclick={() => void chooseVault("change")}
          disabled={busy !== null}
          class={secondaryButtonClass}
        >
          {#if busy === "change"}
            <LoaderCircle size={14} strokeWidth={2.1} class="shrink-0 animate-spin" />
          {:else}
            <FolderOpen size={14} strokeWidth={1.9} class="shrink-0" />
          {/if}
          <span>{t("settings.data.changeFolder")}</span>
        </button>
      </div>

      <div class="flex items-start justify-between gap-4 px-1 py-1 max-[520px]:flex-col max-[520px]:items-stretch max-[520px]:gap-2">
        <div class="min-w-0 flex-1">
          <div class="text-[0.866667rem] text-foreground">{t("settings.data.importFolder")}</div>
          <div class="mt-0.5 text-[0.8rem] leading-5 text-muted-foreground">
            {t("settings.data.importFolderDescription")}
          </div>
        </div>
        <button
          type="button"
          onclick={() => void chooseVault("import")}
          disabled={busy !== null}
          class={secondaryButtonClass}
        >
          {#if busy === "import"}
            <LoaderCircle size={14} strokeWidth={2.1} class="shrink-0 animate-spin" />
          {:else}
            <FolderInput size={14} strokeWidth={1.9} class="shrink-0" />
          {/if}
          <span>{t("settings.data.importFolder")}</span>
        </button>
      </div>
    </div>
  </section>

  {#if error}
    <div class="h-px shrink-0 scale-y-50 bg-border" aria-hidden="true"></div>

    <section class="flex flex-col gap-4">
      <h2 class="px-1 text-[0.866667rem] font-semibold text-foreground">{t("settings.data.statusHeading")}</h2>
      <Notice
        class="px-1 text-[0.8rem]"
        message={error}
        retryLabel={t("settings.data.reloadFolderStatus")}
        onRetry={busy === null ? retryVaultState : undefined}
      />
    </section>
  {/if}
</div>
