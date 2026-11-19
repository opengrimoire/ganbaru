<script lang="ts">
  import type { PairingInvitation, PairingStatus } from "$lib/api/vault-handoff";
  import TooltipHost from "$lib/components/ui/TooltipHost.svelte";
  import { welcomeProgress } from "$lib/vault/welcome-steps";
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
    /** Leaves the linking screen with the vault active by then. */
    onComplete: (activeVaultId: string) => void | Promise<void>;
  } = $props();
</script>

<OnboardingWindowFrame class="app-shell">
  <VaultHandoffOnboardingContent
    platform="desktop"
    {vaultId}
    {initialInvitation}
    {initialStatus}
    backLocked
    progress={welcomeProgress("linking")}
    {onComplete}
  />
  <TooltipHost />
</OnboardingWindowFrame>
