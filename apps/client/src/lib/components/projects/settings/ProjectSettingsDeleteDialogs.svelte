<script lang="ts">
  import ConfirmDialog from "$lib/components/ui/ConfirmDialog.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import type {
    ProjectCustomField,
    ProjectCustomFieldOption,
    ProjectPriorityConfig,
    ProjectStatus,
    ProjectTag,
  } from "$lib/projects/types";

  let {
    pendingDeleteStatus,
    pendingDeletePriority,
    pendingDeleteTag,
    pendingDeleteCustomField,
    pendingDeleteCustomFieldOption,
    customFieldForOption,
    onConfirmDeleteStatus,
    onCancelDeleteStatus,
    onConfirmDeletePriority,
    onCancelDeletePriority,
    onConfirmDeleteTag,
    onCancelDeleteTag,
    onConfirmDeleteCustomField,
    onCancelDeleteCustomField,
    onConfirmDeleteCustomFieldOption,
    onCancelDeleteCustomFieldOption,
  }: {
    pendingDeleteStatus: ProjectStatus | undefined;
    pendingDeletePriority: ProjectPriorityConfig | undefined;
    pendingDeleteTag: ProjectTag | undefined;
    pendingDeleteCustomField: ProjectCustomField | undefined;
    pendingDeleteCustomFieldOption: ProjectCustomFieldOption | undefined;
    customFieldForOption: (option: ProjectCustomFieldOption) => ProjectCustomField | undefined;
    onConfirmDeleteStatus: () => void;
    onCancelDeleteStatus: () => void;
    onConfirmDeletePriority: () => void;
    onCancelDeletePriority: () => void;
    onConfirmDeleteTag: () => void;
    onCancelDeleteTag: () => void;
    onConfirmDeleteCustomField: () => void;
    onCancelDeleteCustomField: () => void;
    onConfirmDeleteCustomFieldOption: () => void;
    onCancelDeleteCustomFieldOption: () => void;
  } = $props();

  const { t } = getLocalization();
</script>

{#if pendingDeleteStatus}
  <ConfirmDialog
    title={t("projects.settings.deleteStatusTitle", pendingDeleteStatus.name)}
    message={t("projects.settings.deleteStatusMessage", pendingDeleteStatus.name)}
    confirmLabel={t("projects.settings.deleteStatusConfirm")}
    cancelLabel={t("common.cancel")}
    onConfirm={onConfirmDeleteStatus}
    onCancel={onCancelDeleteStatus}
  />
{/if}

{#if pendingDeletePriority}
  <ConfirmDialog
    title={t("projects.settings.deletePriorityTitle", pendingDeletePriority.name)}
    message={t("projects.settings.deletePriorityMessage", pendingDeletePriority.name)}
    confirmLabel={t("projects.settings.deletePriorityConfirm")}
    cancelLabel={t("common.cancel")}
    onConfirm={onConfirmDeletePriority}
    onCancel={onCancelDeletePriority}
  />
{/if}

{#if pendingDeleteTag}
  <ConfirmDialog
    title={t("projects.settings.deleteTagTitle", pendingDeleteTag.name)}
    message={t("projects.settings.deleteTagMessage", pendingDeleteTag.name)}
    confirmLabel={t("projects.settings.deleteTagConfirm")}
    cancelLabel={t("common.cancel")}
    onConfirm={onConfirmDeleteTag}
    onCancel={onCancelDeleteTag}
  />
{/if}

{#if pendingDeleteCustomField}
  <ConfirmDialog
    title={t("projects.customFields.deleteFieldTitle", pendingDeleteCustomField.name)}
    message={t("projects.customFields.deleteFieldMessage", pendingDeleteCustomField.name)}
    confirmLabel={t("projects.customFields.deleteFieldConfirm")}
    cancelLabel={t("common.cancel")}
    onConfirm={onConfirmDeleteCustomField}
    onCancel={onCancelDeleteCustomField}
  />
{/if}

{#if pendingDeleteCustomFieldOption}
  <ConfirmDialog
    title={t("projects.customFields.deleteOptionTitle", pendingDeleteCustomFieldOption.name)}
    message={t(
      "projects.customFields.deleteOptionMessage",
      pendingDeleteCustomFieldOption.name,
      customFieldForOption(pendingDeleteCustomFieldOption)?.name ?? "",
    )}
    confirmLabel={t("projects.customFields.deleteOptionConfirm")}
    cancelLabel={t("common.cancel")}
    onConfirm={onConfirmDeleteCustomFieldOption}
    onCancel={onCancelDeleteCustomFieldOption}
  />
{/if}
