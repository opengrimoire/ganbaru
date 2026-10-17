<script lang="ts">
  import { untrack } from "svelte";
  import type { PairingInvitation, PairingStatus } from "$lib/api/vault-handoff";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import {
    canBootstrapUntouchedVault,
    completeVaultHandoffOnboarding,
    markIndependentVaultUsed,
  } from "$lib/vault/handoff-onboarding";
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
  const bootstrapUntouchedVault = untrack(
    () => bootstrapReplicaOnLink && canBootstrapUntouchedVault(safeStorage()),
  );
  const readyToContinue = $derived(
    status?.linked === true
      && (platform === "desktop"
        || status.replicaReady
        || status.canWrite === true
        || !bootstrapUntouchedVault),
  );

  function safeStorage(): Storage | undefined {
    try {
      return window.localStorage;
    } catch {
      return undefined;
    }
  }

  async function leave(persistCompletion: boolean): Promise<void> {
    if (completing) return;
    completing = true;
    try {
      if (!persistCompletion) {
        const storage = safeStorage();
        if (storage) markIndependentVaultUsed(storage);
      }
      await onComplete();
      const storage = safeStorage();
      if (storage) completeVaultHandoffOnboarding(storage, vaultId);
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
<section
  class="mx-auto grid min-h-full w-full max-w-5xl content-center px-4 py-8 min-[560px]:px-8"
  data-vault-handoff-onboarding={platform}
>
  <div class={platform === "desktop"
    ? "mx-auto flex w-full max-w-4xl flex-col gap-6"
    : "mx-auto flex w-full max-w-md flex-col gap-6"}>
    {#if platform === "android"}
      <div class="space-y-2 text-center">
        <h1 class="text-2xl font-semibold leading-tight text-foreground min-[560px]:text-3xl">
          {t("vaultHandoff.androidOnboardingTitle")}
        </h1>
        <p class="text-sm leading-6 text-muted-foreground">
          {t("vaultHandoff.androidOnboardingDescription")}
        </p>
      </div>
    {/if}

    <VaultHandoffPanel
      {platform}
      {initialInvitation}
      {initialStatus}
      bootstrapReplicaOnLink={bootstrapUntouchedVault}
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
          class="min-h-11 self-center rounded-md px-4 py-2 text-sm font-medium text-muted-foreground hover:bg-accent hover:text-foreground"
          onclick={() => void leave(false)}
        >
          {t("vaultHandoff.notNow")}
        </button>
      {/if}
    </div>
  </div>
</section>
{/if}
