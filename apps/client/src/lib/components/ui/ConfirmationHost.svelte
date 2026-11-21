<script lang="ts">
  import { onMount } from "svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import ConfirmDialog from "./ConfirmDialog.svelte";
  import { activeConfirmation, answerConfirmation, registerConfirmationHost } from "./confirmation.svelte";

  const { t } = getLocalization();
  const confirmation = $derived(activeConfirmation());

  onMount(() => registerConfirmationHost());
</script>

{#if confirmation}
  {@const id = confirmation.id}
  {#key id}
    <ConfirmDialog
      title={confirmation.request.title}
      message={confirmation.request.message}
      confirmLabel={confirmation.request.confirmLabel}
      cancelLabel={confirmation.request.cancelLabel ?? t("common.cancel")}
      danger={confirmation.request.danger ?? false}
      onConfirm={() => answerConfirmation(id, true)}
      onCancel={() => answerConfirmation(id, false)}
    />
  {/key}
{/if}
