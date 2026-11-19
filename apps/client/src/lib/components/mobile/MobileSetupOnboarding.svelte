<script lang="ts">
  import { untrack } from "svelte";
  import { completeWelcomeTour } from "$lib/vault/welcome-tour";
  import MobileFocusOnboarding from "./MobileFocusOnboarding.svelte";
  import MobileVaultHandoffOnboarding from "./MobileVaultHandoffOnboarding.svelte";
  import MobileWelcomeTour from "./MobileWelcomeTour.svelte";

  /**
   * The Android onboarding after the vault exists: the feature tour, the access review, then the linking screen, as one
   * screen so moving between them never reloads the app. Each stage shows only while it is pending. The access review
   * stays mounted while linking shows, so Back from linking returns to its last screen as the person left it. Back is
   * offered only when the review had a screen to return to; the tour is left behind once it finishes.
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

  type Stage = "tour" | "access" | "linking";

  let stage = $state<Stage>(untrack(() => (tourPending ? "tour" : accessPending ? "access" : "linking")));
  let accessScreens = $state(0);

  function localStorageOrUndefined(): Storage | undefined {
    try {
      return window.localStorage;
    } catch {
      return undefined;
    }
  }

  function finishTour(): void {
    completeWelcomeTour(localStorageOrUndefined(), vaultId);
    if (accessPending) stage = "access";
    else if (linkingPending) stage = "linking";
    else void onComplete();
  }

  function finishAccess(stepCount: number): void {
    accessScreens = stepCount;
    if (linkingPending) stage = "linking";
    else void onComplete();
  }
</script>

{#if stage === "tour"}
  <MobileWelcomeTour onComplete={finishTour} />
{/if}
{#if accessPending && stage !== "tour"}
  <!-- Hidden and inert while linking shows, so its Enter shortcut and focusable controls stay out of the way. -->
  <div class={stage === "access" ? "contents" : "hidden"} inert={stage !== "access"}>
    <MobileFocusOnboarding onComplete={finishAccess} />
  </div>
{/if}
{#if stage === "linking"}
  <MobileVaultHandoffOnboarding
    {vaultId}
    bootstrapReplicaOnLink
    onBack={accessScreens > 0 ? () => { stage = "access"; } : undefined}
    {onComplete}
  />
{/if}
