<script lang="ts">
  import type { PairingInvitation, PairingStatus } from "$lib/api/vault-handoff";
  import TooltipHost from "$lib/components/ui/TooltipHost.svelte";
  import OnboardingWindowFrame from "../OnboardingWindowFrame.svelte";
  import VaultHandoffOnboardingContent from "./VaultHandoffOnboardingContent.svelte";

  let {
    vaultId,
    initialInvitation,
    initialStatus,
    onComplete,
  }: {
    vaultId: string;
    initialInvitation: PairingInvitation | null;
    initialStatus: PairingStatus;
    onComplete: () => void | Promise<void>;
  } = $props();
</script>

<OnboardingWindowFrame>
  <VaultHandoffOnboardingContent
    platform="desktop"
    {vaultId}
    {initialInvitation}
    {initialStatus}
    {onComplete}
  />
  <TooltipHost />
</OnboardingWindowFrame>
