<script lang="ts">
  import Notice from "$lib/components/ui/Notice.svelte";
  import { formatDateTime, formatNumber } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { formatVaultError, listKeptVaultFolders, type KeptVaultFolder } from "$lib/vault/state";

  /**
   * Lists the whole vault folders Android kept beside the vault in use, so a folder set aside
   * after a lost database or a restore is never invisible on the phone.
   */
  const localization = getLocalization();
  const { t } = localization;
  const BYTES_PER_MEGABYTE = 1_000_000;

  let folders = $state<KeptVaultFolder[]>([]);
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
        <div class="min-w-0 px-1 py-1 text-[0.8rem] leading-5">
          <div class="truncate text-foreground">{folder.displayName ?? t("settings.data.replacedVaultUnnamed")}</div>
          <div class="text-muted-foreground">{folderDetails(folder)}</div>
        </div>
      {/each}
    </div>

    {#if error}
      <Notice class="px-1 text-[0.8rem]" message={error} />
    {/if}
  </section>
{/if}
