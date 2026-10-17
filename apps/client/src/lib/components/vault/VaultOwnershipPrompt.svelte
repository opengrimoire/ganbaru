<script lang="ts">
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import { onMount, tick } from "svelte";
  import ConfirmDialog from "$lib/components/ui/ConfirmDialog.svelte";
  import { readPairingStatus, type PairingStatus } from "$lib/api/vault-handoff";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { hasOnlyShortcutModifier } from "$lib/keyboard-shortcuts";
  import {
    activateModalFocus,
    activateModalKeyboardLayer,
    trapModalTabKey,
  } from "$lib/modal-focus";
  import { BUILD_PLATFORM_PROFILE, platformHasCapability } from "$lib/platform";
  import { getMobileBackStack } from "$lib/stores/mobile-back-stack.svelte";
  import { formatHandoffError } from "$lib/vault/handoff-workflow";
  import { getVaultOwnershipStatus } from "$lib/vault/state";
  import {
    beginVaultOwnershipTransition,
    cancelVaultOwnershipTransition,
    requestVaultOwnership,
    shouldPresentVaultOwnershipPrompt,
    type VaultOwnershipPlatform,
  } from "$lib/vault/ownership-prompt";

  let {
    platform,
    closeConfirmationOpen = false,
    onRequestClose = () => Promise.resolve(),
    onStatusReady = () => undefined,
  }: {
    platform: VaultOwnershipPlatform;
    closeConfirmationOpen?: boolean;
    onRequestClose?: () => Promise<void>;
    onStatusReady?: () => void;
  } = $props();

  const { t } = getLocalization();
  const mobileBackStack = getMobileBackStack();
  const androidSystemBackAvailable = platformHasCapability(
    BUILD_PLATFORM_PROFILE,
    "system.android-back",
  );
  let status = $state<PairingStatus | null>(null);
  let replicatedWrites = $state(false);
  let dismissed = $state(false);
  let switching = $state(false);
  let replacementConfirmationOpen = $state(false);
  let error = $state<string | null>(null);
  let dialog = $state<HTMLDivElement | null>(null);
  let continueButton = $state<HTMLButtonElement | null>(null);
  let previousCanWrite: boolean | null = null;
  let statusReadyReported = false;

  const visible = $derived(
    shouldPresentVaultOwnershipPrompt(status)
      && !dismissed
      && !closeConfirmationOpen,
  );
  const ownerLabel = $derived(
    status?.peerLabel
      ?? t("vaultHandoff.anotherDevice"),
  );
  const description = $derived(t(replicatedWrites ? "vaultOwnershipPrompt.replicaDescription" : "vaultOwnershipPrompt.description"));
  const continueLabel = $derived(
    platform === "desktop"
      ? `${t("vaultOwnershipPrompt.continueReadOnly")} (${t("common.escapeKey")})`
      : t("vaultOwnershipPrompt.continueReadOnly"),
  );

  async function refreshStatus(): Promise<void> {
    try {
      const next = await readPairingStatus();
      if (previousCanWrite === true && next.canWrite === false) dismissed = false;
      previousCanWrite = next.canWrite;
      replicatedWrites = shouldPresentVaultOwnershipPrompt(next) && await readReplicatedWrites();
      status = next;
    } catch (cause) {
      console.warn("Failed to check vault ownership:", cause);
    } finally {
      if (!statusReadyReported) {
        statusReadyReported = true;
        onStatusReady();
      }
    }
  }

  /** Whether Quick notes stay editable here; the prompt falls back to the read-only copy. */
  async function readReplicatedWrites(): Promise<boolean> {
    try {
      return (await getVaultOwnershipStatus()).replicatedWrites;
    } catch (cause) {
      console.warn("Failed to read replicated write access:", cause);
      return false;
    }
  }

  function continueReadOnly(): void {
    if (switching) return;
    dismissed = true;
    error = null;
  }

  async function performOwnershipSwitch(): Promise<void> {
    if (switching) return;
    switching = true;
    error = null;
    beginVaultOwnershipTransition({
      title: t("vaultOwnershipPrompt.title", ownerLabel),
      description,
      actionLabel: t("vaultOwnershipPrompt.switching"),
      secondaryLabel: continueLabel,
    });
    try {
      status = await requestVaultOwnership(platform);
      window.location.reload();
    } catch (cause) {
      cancelVaultOwnershipTransition();
      error = formatHandoffError(cause, t);
    } finally {
      switching = false;
    }
  }

  function useOnThisDevice(): void {
    if (switching) return;
    if (status?.replicaReady === false) {
      replacementConfirmationOpen = true;
      return;
    }
    void performOwnershipSwitch();
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (
      platform === "desktop"
      && hasOnlyShortcutModifier(event, { shift: true })
      && event.key.toLowerCase() === "w"
    ) {
      event.preventDefault();
      void onRequestClose();
      return;
    }
    if (event.key === "Tab" && dialog) {
      trapModalTabKey(dialog, event);
      return;
    }
    if (event.key === "Escape") {
      event.preventDefault();
      continueReadOnly();
      return;
    }
    if (event.key === "Enter") {
      event.preventDefault();
      useOnThisDevice();
      return;
    }
    event.stopPropagation();
  }

  onMount(() => {
    void refreshStatus();
    const timer = window.setInterval(() => {
      if (document.visibilityState === "visible") void refreshStatus();
    }, 5_000);
    const handleVisibility = (): void => {
      if (document.visibilityState === "visible") void refreshStatus();
    };
    document.addEventListener("visibilitychange", handleVisibility);
    return () => {
      window.clearInterval(timer);
      document.removeEventListener("visibilitychange", handleVisibility);
    };
  });

  $effect(() => {
    if (!visible) return;
    const deactivateKeyboard = activateModalKeyboardLayer(handleKeydown);
    const deactivateBack = androidSystemBackAvailable
      ? mobileBackStack.activate({ handle: continueReadOnly })
      : () => undefined;
    let deactivateFocus = (): void => undefined;
    void tick().then(() => {
      if (dialog && visible) deactivateFocus = activateModalFocus(dialog, continueButton);
    });
    return () => {
      deactivateKeyboard();
      deactivateBack();
      deactivateFocus();
    };
  });
</script>

{#if visible}
  <div
    class="fixed z-90 flex items-center justify-center overflow-hidden bg-black/55 px-5 py-8"
    class:android-system-font={platform === "android"}
    style="left: var(--visual-viewport-offset-left); top: var(--visual-viewport-offset-top); width: var(--visual-viewport-width); height: var(--visual-viewport-height); padding-top: calc(var(--safe-area-top) + 2rem); padding-right: calc(var(--safe-area-right) + 1.25rem); padding-bottom: calc(var(--safe-area-bottom) + 2rem); padding-left: calc(var(--safe-area-left) + 1.25rem);"
    data-vault-ownership-prompt
  >
    <div
      bind:this={dialog}
      role="dialog"
      aria-modal="true"
      aria-labelledby="vault-owner-title"
      aria-describedby="vault-owner-description"
      tabindex="-1"
      class="relative z-10 flex max-h-full w-full max-w-lg flex-col items-center overflow-y-auto px-5 py-8 text-center text-white outline-none sm:px-8"
    >
      <h2 id="vault-owner-title" class="vault-owner-legible text-balance text-[1.65rem] font-semibold leading-tight tracking-[-0.035em] text-white/92 sm:text-[1.9rem]">
        {t("vaultOwnershipPrompt.title", ownerLabel)}
      </h2>
      <p id="vault-owner-description" class="vault-owner-legible mt-3 max-w-sm text-balance text-[0.933333rem] leading-6 text-white/78">
        {description}
      </p>

      <div class="mt-7 flex w-full max-w-xs flex-col items-center gap-2">
        <button
          type="button"
          disabled={switching}
          onclick={useOnThisDevice}
          class="flex min-h-12 w-full items-center justify-center gap-2 rounded-lg bg-white px-5 text-[0.9rem] font-semibold text-[#111217] shadow-[0_2px_8px_rgba(0,0,0,0.10)] transition-[background-color,transform] hover:bg-white/90 active:scale-[0.985] disabled:pointer-events-none disabled:opacity-65"
        >
          {#if switching}
            <LoaderCircle size={17} strokeWidth={2} class="animate-spin" aria-hidden="true" />
            {t("vaultOwnershipPrompt.switching")}
          {:else}
            {platform === "desktop"
              ? `${t("vaultHandoff.useHere")} (${t("common.enterKey")})`
              : t("vaultHandoff.useHere")}
          {/if}
        </button>
        <button
          bind:this={continueButton}
          type="button"
          disabled={switching}
          onclick={continueReadOnly}
          class="min-h-11 rounded-lg px-5 text-[0.866667rem] font-medium text-white/68 transition-colors hover:text-white disabled:pointer-events-none disabled:opacity-40"
        >
          {continueLabel}
        </button>
      </div>

      {#if error}
        <p role="alert" class="vault-owner-legible mt-4 max-w-sm text-[0.8rem] leading-5 text-red-200">
          {error}
        </p>
      {/if}
    </div>
  </div>
{/if}

{#if replacementConfirmationOpen}
  <ConfirmDialog
    title={t("vaultOwnershipPrompt.replacementTitle")}
    message={platform === "android"
      ? t("vaultOwnershipPrompt.replacementAndroid")
      : t("vaultOwnershipPrompt.replacementDesktop")}
    confirmLabel={t("vaultOwnershipPrompt.replacementConfirm")}
    cancelLabel={t("common.cancel")}
    danger={false}
    onConfirm={() => {
      replacementConfirmationOpen = false;
      void performOwnershipSwitch();
    }}
    onCancel={() => { replacementConfirmationOpen = false; }}
  />
{/if}

<style>
  .android-system-font {
    font-family: ui-sans-serif, system-ui, sans-serif;
  }

  .vault-owner-legible {
    text-shadow: 0 1px 3px rgb(0 0 0 / 0.45);
  }
</style>
