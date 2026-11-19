<script lang="ts">
  import { untrack } from "svelte";
  import { carryWelcomeTour, completeWelcomeTour, welcomeTourPending } from "$lib/vault/welcome-tour";
  import MobileFocusOnboarding from "./MobileFocusOnboarding.svelte";
  import MobileVaultHandoffOnboarding from "./MobileVaultHandoffOnboarding.svelte";
  import MobileWelcomeTour from "./MobileWelcomeTour.svelte";

  /**
   * The Android onboarding after the vault exists: the linking screen, the feature tour, then the access review, as one
   * screen so moving between them never reloads the app. Each stage shows only while it is pending. Linking comes first
   * so a phone that joins an existing vault does so before anything else in onboarding writes to the starter vault.
   * Its Back shows disabled, since the vault is already chosen.
   */
  let {
    vaultId,
    tourPending,
    accessPending,
    linkingPending,
    onComplete,
  }: {
    vaultId: string;
    /** Whether the feature tour still has to show for this vault. */
    tourPending: boolean;
    /** Whether the access review still has to run. */
    accessPending: boolean;
    /** Whether the linking screen still has to show for this vault. */
    linkingPending: boolean;
    onComplete: () => void | Promise<void>;
  } = $props();

  type Stage = "linking" | "tour" | "access";

  /** The vault the tour belongs to, which a linked copy may replace while the linking screen shows. */
  let tourVaultId = $state(untrack(() => vaultId));
  let stage = $state<Stage>(untrack(() => (linkingPending ? "linking" : tourPending ? "tour" : "access")));

  function localStorageOrUndefined(): Storage | undefined {
    try {
      return window.localStorage;
    } catch {
      return undefined;
    }
  }

  function finishLinking(activeVaultId: string): void {
    const storage = localStorageOrUndefined();
    carryWelcomeTour(storage, vaultId, activeVaultId);
    tourVaultId = activeVaultId;
    if (welcomeTourPending(storage, activeVaultId)) stage = "tour";
    else afterTour();
  }

  function finishTour(): void {
    completeWelcomeTour(localStorageOrUndefined(), tourVaultId);
    afterTour();
  }

  function afterTour(): void {
    if (accessPending) stage = "access";
    else void onComplete();
  }
</script>

{#if stage === "linking"}
  <MobileVaultHandoffOnboarding
    {vaultId}
    bootstrapReplicaOnLink
    backLocked
    onComplete={finishLinking}
  />
{:else if stage === "tour"}
  <MobileWelcomeTour onComplete={finishTour} />
{:else}
  <MobileFocusOnboarding onComplete={() => void onComplete()} />
{/if}
