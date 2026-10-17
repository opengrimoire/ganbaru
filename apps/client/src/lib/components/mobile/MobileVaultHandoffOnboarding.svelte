<script lang="ts">
  import type { PairingStatus } from "$lib/api/vault-handoff";
  import VaultHandoffOnboardingContent from "$lib/components/vault/handoff/VaultHandoffOnboardingContent.svelte";
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
  class="mobile-viewport-height w-screen overflow-y-auto bg-background pb-(--safe-area-bottom) pt-(--safe-area-top) text-foreground"
  style="padding-left: var(--safe-area-left); padding-right: var(--safe-area-right);"
>
  <VaultHandoffOnboardingContent
    platform="android"
    {vaultId}
    {initialStatus}
    {bootstrapReplicaOnLink}
    {onComplete}
  />
</main>
