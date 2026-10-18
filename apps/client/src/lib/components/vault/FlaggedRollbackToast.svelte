<script lang="ts">
  import ActionToast from "$lib/components/ui/ActionToast.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getSettingsLauncher } from "$lib/stores/settings-launcher.svelte";
  import { listDatabaseRollbacks, pendingFlaggedRollbacks } from "$lib/vault/state";

  /**
   * Tells the user at startup that a handoff replaced this vault with drastically less data, and
   * opens Settings > Data, where the kept copy can be restored or its notice dismissed.
   */
  const { t } = getLocalization();
  const settingsLauncher = getSettingsLauncher();
  let visible = $state(false);

  async function checkFlaggedRollbacks(): Promise<void> {
    try {
      visible = pendingFlaggedRollbacks(await listDatabaseRollbacks()).length > 0;
    } catch (cause: unknown) {
      console.error("Failed to list kept vault copies:", cause);
    }
  }

  function review(): void {
    visible = false;
    settingsLauncher.open("data");
  }

  $effect(() => {
    void checkFlaggedRollbacks();
  });
</script>

{#if visible}
  <ActionToast
    message={t("settings.data.rollbackFlaggedToast")}
    variant="error"
    actionLabel={t("settings.data.rollbackReview")}
    onAction={review}
    onDismiss={() => visible = false}
  />
{/if}
