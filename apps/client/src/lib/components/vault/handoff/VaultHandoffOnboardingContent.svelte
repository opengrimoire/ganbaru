<script lang="ts">
  import { untrack } from "svelte";
  import { fade } from "svelte/transition";
  import {
    releaseStarterVault,
    type PairingInvitation,
    type PairingStatus,
  } from "$lib/api/vault-handoff";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { cn } from "$lib/utils";
  import { dismissOnOutside } from "$lib/utils/dismiss-on-outside";
  import { completeVaultHandoffOnboarding } from "$lib/vault/handoff-onboarding";
  import { getActiveVaultInfo } from "$lib/vault/state";
  import OnboardingNav, { type OnboardingNavPrimary, type OnboardingNavProgress } from "../OnboardingNav.svelte";
  import VaultHandoffPanel from "./VaultHandoffPanel.svelte";

  /**
   * The linking screen of first use, with the shared onboarding navigation at its bottom: its primary action continues
   * without linking until a device links. With `backLocked`, Back shows disabled, for first use, where the screens
   * before linking cannot be chosen again. With `progress`, the navigation shows the screen's place among the first-use
   * screens. `onComplete` receives the vault active once the screen is left, which a linked or server copy may have
   * replaced.
   */
  let {
    platform,
    vaultId,
    initialInvitation = null,
    initialStatus = null,
    bootstrapReplicaOnLink = false,
    backLocked = false,
    progress,
    onComplete,
  }: {
    platform: "desktop" | "android";
    vaultId: string;
    initialInvitation?: PairingInvitation | null;
    initialStatus?: PairingStatus | null;
    bootstrapReplicaOnLink?: boolean;
    backLocked?: boolean;
    progress?: OnboardingNavProgress;
    onComplete: (activeVaultId: string) => void | Promise<void>;
  } = $props();

  const { t } = getLocalization();
  let status = $state<PairingStatus | null>(untrack(() => initialStatus));
  let completing = $state(false);
  /**
   * Whether continuing without linking first asks the user to acknowledge that the vault lives only on this device.
   * Android keeps the vault in app storage, which uninstalling removes; a desktop vault is a folder the user keeps.
   */
  const confirmsUnlinkedContinue = $derived(platform === "android");
  let unlinkedNoticeRequested = $state(false);
  const linked = $derived(status?.linked === true);
  const unlinkedNoticeOpen = $derived(unlinkedNoticeRequested && !linked && !completing);
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
      ? { label: t("vaultHandoff.continueWithoutLinking"), onClick: continueWithoutLinking, busy: completing }
      : { label: t("vaultHandoff.continue"), onClick: () => void leave(true), busy: completing || !readyToContinue },
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

  function continueWithoutLinking(): void {
    if (confirmsUnlinkedContinue) unlinkedNoticeRequested = true;
    else void leave(false);
  }

  /** Moves focus to the notice's acknowledgement so keyboard and screen reader users land on it. */
  function focusOnMount(node: HTMLElement): void {
    node.focus();
  }

  async function leave(persistCompletion: boolean): Promise<void> {
    if (completing) return;
    completing = true;
    try {
      if (!persistCompletion) await releaseStarterVault();
      const activeId = persistCompletion ? await activeVaultId() : null;
      await onComplete(activeId ?? vaultId);
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

  <div class="relative">
    {#if unlinkedNoticeOpen}
      <div class="pointer-events-none absolute inset-x-0 bottom-full z-40 flex justify-center px-4">
        <div
          use:dismissOnOutside={{ onDismiss: () => { unlinkedNoticeRequested = false; } }}
          transition:fade={{ duration: 120 }}
          role="dialog"
          aria-labelledby="unlinked-notice-text"
          class="surface-floating pointer-events-auto flex w-floating-lg flex-col gap-3 p-4"
        >
          <p id="unlinked-notice-text" class="text-sm text-foreground">{t("vaultHandoff.unlinkedNotice")}</p>
          <button
            use:focusOnMount
            type="button"
            class="inline-flex min-h-11 items-center justify-center rounded-md bg-primary px-4 py-2 text-sm font-medium text-primary-foreground transition-colors hover:bg-primary/90 active:bg-primary/90"
            onclick={() => void leave(false)}
          >
            {t("vaultHandoff.unlinkedNoticeAcknowledge")}
          </button>
        </div>
      </div>
    {/if}
    <OnboardingNav {primary} {backLocked} {progress} />
  </div>
</div>
