<script lang="ts">
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import OnboardingWindowFrame from "./OnboardingWindowFrame.svelte";
  import VaultWelcomeContent from "./VaultWelcomeContent.svelte";

  /**
   * The desktop feature tour, once the setup screens have opened the vault. It starts on the first feature with no
   * Back to the setup screens, since the vault is already chosen, and `onComplete` moves on to the linking screen.
   */
  let { onComplete }: { onComplete: () => void | Promise<void> } = $props();

  const { t } = getLocalization();
  let finishing = $state(false);

  async function finish(): Promise<void> {
    if (finishing) return;
    finishing = true;
    try {
      await onComplete();
    } catch (error) {
      console.error("Could not continue after the welcome tour:", error);
      finishing = false;
    }
  }
</script>

<OnboardingWindowFrame class="app-shell">
  <VaultWelcomeContent
    stage="tour"
    last={{
      label: t("vaultSetup.continue"),
      onClick: () => void finish(),
      busy: finishing,
      disabled: finishing,
    }}
    lastBackDisabled={finishing}
  />
</OnboardingWindowFrame>
