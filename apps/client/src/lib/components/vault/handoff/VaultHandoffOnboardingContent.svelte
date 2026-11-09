<script lang="ts">
  import { untrack } from "svelte";
  import {
    releaseStarterVault,
    type PairingInvitation,
    type PairingStatus,
  } from "$lib/api/vault-handoff";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { cn } from "$lib/utils";
  import { completeVaultHandoffOnboarding } from "$lib/vault/handoff-onboarding";
  import { getActiveVaultInfo } from "$lib/vault/state";
  import OnboardingNav, { type OnboardingNavPrimary } from "../OnboardingNav.svelte";
  import VaultHandoffPanel from "./VaultHandoffPanel.svelte";

  /**
   * The linking screen of first use, with the shared onboarding navigation at its bottom: its primary action continues
   * without linking until a device links, and Back, when the shell passes `onBack`, returns to the screen before it
   * until a device links.
   */
  let {
    platform,
    vaultId,
    initialInvitation = null,
    initialStatus = null,
    bootstrapReplicaOnLink = false,
    onBack,
    onComplete,
  }: {
    platform: "desktop" | "android";
    vaultId: string;
    initialInvitation?: PairingInvitation | null;
    initialStatus?: PairingStatus | null;
    bootstrapReplicaOnLink?: boolean;
    onBack?: () => void;
    onComplete: () => void | Promise<void>;
  } = $props();

  const { t } = getLocalization();
  let status = $state<PairingStatus | null>(untrack(() => initialStatus));
  let completing = $state(false);
  const linked = $derived(status?.linked === true);
  const readyToContinue = $derived(
    status?.linked === true
      && (platform === "desktop"
        || status.replicaReady
        || !bootstrapReplicaOnLink),
  );
  /**
   * Continues without linking until a device links; once linked, continues as soon as this device holds its copy of the
   * vault, waiting while the copy arrives.
   */
  const primary = $derived<OnboardingNavPrimary>(
    !linked
      ? { label: t("vaultHandoff.continueWithoutLinking"), onClick: () => void leave(false) }
      : { label: t("vaultHandoff.continue"), onClick: () => void leave(true), busy: !readyToContinue },
  );

  function safeStorage(): Storage | undefined {
    try {
      return window.localStorage;
    } catch {
      return undefined;
    }
  }

  /**
   * The vault that is active now, which differs from `vaultId` once a linked or server copy
   * replaced the vault onboarding started with.
   */
  async function activeVaultId(): Promise<string | null> {
    try {
      return (await getActiveVaultInfo())?.vaultId ?? null;
    } catch (error) {
      console.warn("the active vault could not be read after onboarding", error);
      return null;
    }
  }

  async function leave(persistCompletion: boolean): Promise<void> {
    if (completing) return;
    completing = true;
    try {
      if (!persistCompletion) await releaseStarterVault();
      const activeId = persistCompletion ? await activeVaultId() : null;
      await onComplete();
      const storage = safeStorage();
      if (storage) {
        completeVaultHandoffOnboarding(storage, vaultId);
        if (activeId && activeId !== vaultId) completeVaultHandoffOnboarding(storage, activeId);
      }
    } catch (error) {
      completing = false;
      console.error("Failed to leave linking onboarding:", error);
    }
  }
</script>

{#if completing}
  <section
    class="grid h-full w-full place-items-center"
    role="status"
    aria-label={t("common.loading")}
  >
    <svg
      viewBox="0 0 48 48"
      class="ganbaru-loading-ring size-12 text-foreground"
      aria-hidden="true"
    >
      <circle
        class="ganbaru-loading-ring-stroke"
        cx="24"
        cy="24"
        r="18"
        fill="none"
        stroke="currentColor"
        stroke-width="3"
        stroke-linecap="round"
      />
    </svg>
  </section>
{:else}
<div class="@container flex h-full flex-col" data-vault-handoff-onboarding={platform}>
  <!-- The content hangs from the top, so the source choices in the title stay put while the panel below changes height. Android starts
       it higher so the tallest panel fits a phone screen without scrolling. -->
  <section
    class={cn(
      "min-h-0 flex-1 overflow-x-hidden overflow-y-auto px-4 pb-6 min-[560px]:px-8",
      platform === "android" ? "pt-[max(2rem,7vh)]" : "pt-[max(2rem,12vh)]",
    )}
  >
    <div class={platform === "desktop"
      ? "mx-auto flex w-full max-w-4xl flex-col gap-3"
      : "mx-auto flex w-full max-w-md flex-col gap-3"}>
      <VaultHandoffPanel
        {platform}
        {initialInvitation}
        {initialStatus}
        {bootstrapReplicaOnLink}
        presentation="onboarding"
        onStatusChange={(next) => { status = next; }}
        onActivated={() => void leave(true)}
      />
    </div>
  </section>

  <OnboardingNav {primary} onBack={linked ? undefined : onBack} />
</div>
{/if}
