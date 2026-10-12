<script lang="ts">
  import ConfirmDialog from "$lib/components/ui/ConfirmDialog.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { formatNumber } from "$lib/i18n/formatters";
  import type { NotesDatabaseDeletionController } from "$lib/stores/notes/database-deletion.svelte";

  let { controller }: { controller: NotesDatabaseDeletionController } = $props();
  const localization = getLocalization();
  const { t } = localization;
  const current = $derived(controller.prompt);
</script>

{#if current}
  <ConfirmDialog
    title={t(current.loading || current.dataSourceCount > 0 ? "notes.databaseTrashTitle" : "notes.databaseLinkedTrashTitle")}
    message={current.loading ? t("notes.databaseTrashInspecting")
      : current.error ? t("notes.databaseTrashInspectFailed", current.error)
      : current.dataSourceCount > 0 ? t("notes.databaseTrashMessage", current.dataSourceCount, formatNumber(localization.locale, current.dataSourceCount))
      : t("notes.databaseLinkedTrashMessage")}
    confirmLabel={current.error ? t("common.retry") : t("notes.databaseMoveToTrash")}
    cancelLabel={t("common.cancel")}
    onConfirm={() => current.error ? controller.retry() : controller.confirm()}
    onCancel={controller.cancel}
  />
{/if}
