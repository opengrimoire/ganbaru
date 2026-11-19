<script lang="ts">
  import type { PairingStatus } from "$lib/api/vault-handoff";
  import VaultHandoffOnboardingContent from "$lib/components/vault/handoff/VaultHandoffOnboardingContent.svelte";
  import MobileOnboardingProgress from "./MobileOnboardingProgress.svelte";
  import { requireActiveVaultIdentity } from "$lib/vault/active-vault";

  let {
    vaultId = requireActiveVaultIdentity(),
    initialStatus = null,
    bootstrapReplicaOnLink = false,
    onBack,
    backLocked = false,
    onComplete,
  }: {
    vaultId?: string;
    initialStatus?: PairingStatus | null;
    bootstrapReplicaOnLink?: boolean;
    /** Returns to the screen before linking; without it, the screen has no Back. */
    onBack?: () => void;
    /** Shows Back disabled in first use, where the screens before linking cannot be chosen again. */
    backLocked?: boolean;
    /** Leaves the screen with the vault active by then, which a linked copy may have replaced. */
    onComplete: (activeVaultId: string) => void | Promise<void>;
  } = $props();
</script>

<main
  class="mobile-viewport-height flex w-screen flex-col overflow-hidden bg-background pb-(--safe-area-bottom) pt-(--safe-area-top) text-foreground"
  style="padding-left: var(--safe-area-left); padding-right: var(--safe-area-right);"
>
  <MobileOnboardingProgress stage="linking">
    <div class="h-full">
      <VaultHandoffOnboardingContent
        platform="android"
        {vaultId}
        {initialStatus}
        {bootstrapReplicaOnLink}
        {onBack}
        {backLocked}
        {onComplete}
      />
    </div>
  </MobileOnboardingProgress>
</main>
