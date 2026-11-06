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
  import VaultHandoffPanel from "./VaultHandoffPanel.svelte";
  import { onboardingPrimaryAction } from "$lib/vault/onboarding-primary-action";

  let {
    platform,
    vaultId,
    initialInvitation = null,
    initialStatus = null,
    bootstrapReplicaOnLink = false,
    onComplete,
  }: {
    platform: "desktop" | "android";
    vaultId: string;
    initialInvitation?: PairingInvitation | null;
    initialStatus?: PairingStatus | null;
    bootstrapReplicaOnLink?: boolean;
    onComplete: () => void | Promise<void>;
  } = $props();

  const { t } = getLocalization();
  let status = $state<PairingStatus | null>(untrack(() => initialStatus));
  let completing = $state(false);
  const readyToContinue = $derived(
    status?.linked === true
      && (platform === "desktop"
        || status.replicaReady
        || !bootstrapReplicaOnLink),
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
<!-- The content hangs from the top, so the source choices in the title stay put while the panel below changes height. Android starts
     it higher so the tallest panel fits a phone screen without scrolling. -->
<section
  class={cn(
    "mx-auto grid min-h-full w-full max-w-5xl content-start px-4 pb-8 min-[560px]:px-8",
    platform === "android" ? "pt-[max(2rem,7vh)]" : "pt-[max(2rem,12vh)]",
  )}
  data-vault-handoff-onboarding={platform}
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

    <div class="flex flex-col gap-2">
      {#if readyToContinue}
        <button
          use:onboardingPrimaryAction
          type="button"
          class="min-h-11 rounded-md bg-primary px-3 py-2 text-sm font-medium text-primary-foreground hover:bg-primary/90"
          onclick={() => void leave(true)}
        >
          {t("vaultHandoff.continue")}
        </button>
      {/if}
      {#if status?.linked !== true}
        <button
          use:onboardingPrimaryAction
          type="button"
          class="min-h-9 self-center rounded-md px-4 py-1.5 text-sm pointer-coarse:min-h-11 font-medium text-muted-foreground hover:bg-accent hover:text-foreground"
          onclick={() => void leave(false)}
        >
          {t("vaultHandoff.notNow")}
        </button>
      {/if}
    </div>
  </div>
</section>
{/if}
