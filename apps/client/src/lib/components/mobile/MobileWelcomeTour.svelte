<script lang="ts">
  import VaultWelcomeContent from "$lib/components/vault/VaultWelcomeContent.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import MobileOnboardingProgress from "./MobileOnboardingProgress.svelte";

  /**
   * The Android feature tour, once the setup screens have opened the vault. It starts on the first feature with no
   * Back to the setup screens, since the vault is already chosen, and `onComplete` moves on to the access review.
   */
  let { onComplete }: { onComplete: () => void } = $props();

  const { t } = getLocalization();
  let step = $state(0);
</script>

<main
  class="mobile-viewport-height flex w-screen flex-col overflow-hidden bg-background pb-(--safe-area-bottom) pt-(--safe-area-top) text-foreground"
  style="padding-left: var(--safe-area-left); padding-right: var(--safe-area-right);"
>
  <MobileOnboardingProgress stage="tour" screen={step}>
    <VaultWelcomeContent
      stage="tour"
      bind:step
      showStepDots={false}
      last={{ label: t("vaultSetup.continue"), onClick: onComplete }}
    />
  </MobileOnboardingProgress>
</main>
