<script lang="ts">
  import { untrack } from "svelte";
  import MobileFocusOnboarding from "./MobileFocusOnboarding.svelte";
  import MobileVaultHandoffOnboarding from "./MobileVaultHandoffOnboarding.svelte";

  /**
   * The Android onboarding after the vault exists: the access review, then the linking screen, as one screen so moving
   * between them never reloads the app. The access review stays mounted while linking shows, so Back from linking
   * returns to its last screen as the person left it. Back is offered only when the review had a screen to return to.
   */
  let {
    vaultId,
    accessPending,
    linkingPending,
    onComplete,
  }: {
    vaultId: string;
    /** Whether the access review still has to run. */
    accessPending: boolean;
    /** Whether the linking screen still has to show for this vault. */
    linkingPending: boolean;
    onComplete: () => void | Promise<void>;
  } = $props();

  type Stage = "access" | "linking";

  let stage = $state<Stage>(untrack(() => (accessPending ? "access" : "linking")));
  let accessScreens = $state(0);

  function finishAccess(stepCount: number): void {
    accessScreens = stepCount;
    if (linkingPending) stage = "linking";
    else void onComplete();
  }
</script>

{#if accessPending}
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
