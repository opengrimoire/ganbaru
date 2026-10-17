<script lang="ts">
  import ConfirmDialog from "$lib/components/ui/ConfirmDialog.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";

  let {
    element = $bindable(),
    onCancel,
    onConfirm,
  }: {
    element?: HTMLDivElement;
    onCancel: () => void;
    onConfirm: () => void;
  } = $props();

  const { t } = getLocalization();
</script>

<ConfirmDialog
  bind:element
  title={t("chat.composer.changeProviderTitle")}
  message={t("chat.composer.changeProviderDescription")}
  confirmLabel={t("chat.composer.startProviderFork")}
  cancelLabel={t("chat.cancel")}
  {onConfirm}
  {onCancel}
/>
