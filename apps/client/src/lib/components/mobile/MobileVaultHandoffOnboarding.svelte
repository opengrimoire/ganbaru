<script lang="ts">
  import type { PairingStatus } from "$lib/api/vault-handoff";
  import VaultHandoffOnboardingContent from "$lib/components/vault/handoff/VaultHandoffOnboardingContent.svelte";
  import MobileOnboardingProgress from "./MobileOnboardingProgress.svelte";
  import { requireActiveVaultIdentity } from "$lib/vault/active-vault";

  let {
    vaultId = requireActiveVaultIdentity(),
    initialStatus = null,
    bootstrapReplicaOnLink = false,
    onComplete,
  }: {
    vaultId?: string;
    initialStatus?: PairingStatus | null;
    bootstrapReplicaOnLink?: boolean;
    onComplete: () => void | Promise<void>;
  } = $props();
</script>

<main
  class="mobile-viewport-height flex w-screen flex-col overflow-hidden bg-background pb-(--safe-area-bottom) pt-(--safe-area-top) text-foreground"
  style="padding-left: var(--safe-area-left); padding-right: var(--safe-area-right);"
>
  <MobileOnboardingProgress stage="linking">
    <div class="h-full overflow-y-auto">
      <VaultHandoffOnboardingContent
        platform="android"
        {vaultId}
        {initialStatus}
        {bootstrapReplicaOnLink}
        {onComplete}
      />
    </div>
  </MobileOnboardingProgress>
</main>
