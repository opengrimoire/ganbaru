<script lang="ts">
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import RotateCcw from "@lucide/svelte/icons/rotate-ccw";
  import ConfirmDialog from "$lib/components/ui/ConfirmDialog.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { formatDateTime, formatNumber } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import {
    formatVaultError,
    listKeptVaultFolders,
    putBackKeptVaultFolder,
    type KeptVaultFolder,
  } from "$lib/vault/state";

  /**
   * Lists the whole vault folders Android kept beside the vault in use, so a folder set aside
   * after a lost database or a restore is never invisible on the phone, and puts one back in
   * place of the vault in use.
   *
   * @param onRestored - Reloads the app after a kept folder replaced the active vault.
   */
  let { onRestored }: { onRestored: () => unknown } = $props();

  const localization = getLocalization();
  const { t } = localization;
  const BYTES_PER_MEGABYTE = 1_000_000;
  const buttonClass =
    "inline-flex h-(--control-height) shrink-0 items-center justify-center gap-1.5 rounded-md border border-border bg-card px-2.5 text-[0.8rem] font-medium text-foreground transition-colors hover:bg-accent focus:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:pointer-events-none disabled:opacity-55 dark:bg-transparent";

  let folders = $state<KeptVaultFolder[]>([]);
  let pendingPutBack = $state<KeptVaultFolder | null>(null);
  let busy = $state<string | null>(null);
  let error = $state<string | null>(null);

  async function loadFolders(): Promise<void> {
    try {
      folders = await listKeptVaultFolders();
    } catch (cause: unknown) {
      error = formatVaultError(cause, "general", t);
    }
  }

  function folderDetails(folder: KeptVaultFolder): string {
    const date = formatDateTime(localization.locale, new Date(folder.keptAtMs), {
      dateStyle: "medium",
      timeStyle: "short",
    });
    const size = formatNumber(localization.locale, folder.sizeBytes / BYTES_PER_MEGABYTE, {
      style: "unit",
      unit: "megabyte",
      maximumFractionDigits: 1,
    });
    return `${date} · ${size}`;
  }

  async function putBack(folder: KeptVaultFolder): Promise<void> {
    pendingPutBack = null;
    busy = folder.id;
    error = null;
    try {
      await putBackKeptVaultFolder(folder.id);
      onRestored();
    } catch (cause: unknown) {
      error = formatVaultError(cause, "restore", t);
      await loadFolders();
    } finally {
      busy = null;
    }
  }

  $effect(() => {
    void loadFolders();
  });
</script>

{#if folders.length > 0 || error}
  <div class="h-px shrink-0 bg-border/50" aria-hidden="true"></div>

  <section class="flex flex-col gap-4">
    <div class="px-1">
      <h2 class="text-[0.866667rem] font-semibold text-foreground">{t("settings.data.keptVaultFoldersHeading")}</h2>
      <p class="mt-0.5 text-[0.8rem] leading-5 text-muted-foreground">{t("settings.data.keptVaultFoldersDescription")}</p>
    </div>

    <div class="flex flex-col gap-3">
      {#each folders as folder (folder.id)}
        <div class="flex flex-wrap items-center justify-between gap-x-4 gap-y-2 px-1 py-1">
          <div class="min-w-0 flex-1 text-[0.8rem] leading-5">
            <div class="truncate text-foreground">{folder.displayName ?? t("settings.data.replacedVaultUnnamed")}</div>
            <div class="text-muted-foreground">{folderDetails(folder)}</div>
          </div>
          <button
            type="button"
            onclick={() => pendingPutBack = folder}
            disabled={busy !== null}
            class={buttonClass}
          >
            {#if busy === folder.id}
              <LoaderCircle size={14} strokeWidth={2.1} class="shrink-0 animate-spin" />
            {:else}
              <RotateCcw size={14} strokeWidth={1.9} class="shrink-0" />
            {/if}
            <span>{t("settings.data.keptVaultPutBack")}</span>
          </button>
        </div>
      {/each}
    </div>

    {#if error}
      <Notice class="px-1 text-[0.8rem]" message={error} />
    {/if}
  </section>
{/if}

{#if pendingPutBack}
  <ConfirmDialog
    title={t("settings.data.keptVaultPutBackTitle")}
    message={t("settings.data.keptVaultPutBackMessage")}
    confirmLabel={t("settings.data.keptVaultPutBack")}
    cancelLabel={t("common.cancel")}
    onConfirm={() => { const folder = pendingPutBack; if (folder) void putBack(folder); }}
    onCancel={() => pendingPutBack = null}
  />
{/if}
