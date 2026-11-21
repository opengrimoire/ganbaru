<script lang="ts">
  import type { PairingStatus } from "$lib/api/vault-handoff";
  import VaultHandoffOnboardingContent from "$lib/components/vault/handoff/VaultHandoffOnboardingContent.svelte";
  import MobileOnboardingProgress from "./MobileOnboardingProgress.svelte";
  import { requireActiveVaultIdentity } from "$lib/vault/active-vault";

  /**
   * The Android linking screen. In first use (`firstUse`) it shows the onboarding progress and Back disabled, since the
   * screens before it cannot be chosen again; opened later from the linked-device control, it shows neither.
   */
  let {
    vaultId = requireActiveVaultIdentity(),
    initialStatus = null,
    bootstrapReplicaOnLink = false,
    firstUse = false,
    onComplete,
  }: {
    vaultId?: string;
    initialStatus?: PairingStatus | null;
    bootstrapReplicaOnLink?: boolean;
    firstUse?: boolean;
    /** Leaves the screen with the vault active by then, which a linked copy may have replaced. */
    onComplete: (activeVaultId: string) => void | Promise<void>;
  } = $props();
</script>

{#snippet content()}
  <div class="h-full">
    <VaultHandoffOnboardingContent
      platform="android"
      {vaultId}
      {initialStatus}
      {bootstrapReplicaOnLink}
      backLocked={firstUse}
      {onComplete}
    />
  </div>
{/snippet}

<main
  class="mobile-viewport-height flex w-screen flex-col overflow-hidden bg-background pb-(--safe-area-bottom) pt-(--safe-area-top) text-foreground"
  style="padding-left: var(--safe-area-left); padding-right: var(--safe-area-right);"
>
  {#if firstUse}
    <MobileOnboardingProgress stage="linking">
      {@render content()}
    </MobileOnboardingProgress>
  {:else}
    {@render content()}
  {/if}
</main>
