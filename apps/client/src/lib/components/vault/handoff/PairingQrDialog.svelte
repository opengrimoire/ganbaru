<script lang="ts">
  import X from "@lucide/svelte/icons/x";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import Copy from "@lucide/svelte/icons/copy";
  import { onMount, tick } from "svelte";
  import type {
    DesktopNetworkAccess,
    PairingInvitation,
    PendingEnrollment,
  } from "$lib/api/vault-handoff";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import {
    activateModalFocus,
    activateModalKeyboardLayer,
    trapModalTabKey,
  } from "$lib/modal-focus";
  import EnrollmentApproval from "./EnrollmentApproval.svelte";
  import PairingQrCode from "./PairingQrCode.svelte";

  let {
    invitation,
    networkAccess,
    networkBusy,
    networkError,
    invitationError,
    pendingEnrollment,
    approvalBusy,
    approvalError,
    onGrantNetworkAccess,
    onRefresh,
    onRetry,
    onRespondEnrollment,
    onClose,
  }: {
    invitation: PairingInvitation | null;
    networkAccess: DesktopNetworkAccess | null;
    networkBusy: boolean;
    networkError: string | null;
    invitationError: string | null;
    pendingEnrollment: PendingEnrollment | null;
    approvalBusy: boolean;
    approvalError: string | null;
    onGrantNetworkAccess: () => void;
    onRefresh: () => void;
    onRetry: () => unknown;
    onRespondEnrollment: (approve: boolean) => void;
    onClose: () => void;
  } = $props();

  const { t } = getLocalization();
  let dialog = $state<HTMLDivElement | null>(null);
  let closeButton = $state<HTMLButtonElement | null>(null);
  let copied = $state(false);
  let copyFailed = $state(false);

  async function copyCode(): Promise<void> {
    if (!invitation) return;
    copyFailed = false;
    try {
      await navigator.clipboard.writeText(invitation.invitation);
      copied = true;
      window.setTimeout(() => { copied = false; }, 2_000);
    } catch {
      copyFailed = true;
    }
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (event.key === "Tab" && dialog) {
      trapModalTabKey(dialog, event);
      event.stopPropagation();
      return;
    }
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      onClose();
      return;
    }
    event.stopPropagation();
  }

  onMount(() => {
    const deactivateKeyboard = activateModalKeyboardLayer(handleKeydown);
    let deactivateFocus = (): void => undefined;
    void tick().then(() => {
      if (dialog) deactivateFocus = activateModalFocus(dialog, closeButton);
    });
    return () => {
      deactivateKeyboard();
      deactivateFocus();
    };
  });
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="fixed inset-0 z-90 flex items-center justify-center p-4" onclick={onClose}>
  <div class="surface-backdrop absolute inset-0"></div>
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    bind:this={dialog}
    class="relative z-10 flex max-h-full w-full max-w-md flex-col overflow-y-auto surface-dialog px-5 py-5 outline-none sm:px-8"
    role="dialog"
    aria-modal="true"
    aria-labelledby="pairing-qr-title"
    tabindex="-1"
    onclick={(event) => event.stopPropagation()}
    onkeydown={handleKeydown}
  >
    <div>
      <div class="flex items-center justify-between gap-3">
        <h2 id="pairing-qr-title" class="min-w-0 text-base font-semibold text-foreground">
          {t("vaultHandoff.qrDialogTitle")}
        </h2>
        <button
          bind:this={closeButton}
          type="button"
          class="flex size-8 shrink-0 items-center justify-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground"
          aria-label={t("common.close")}
          onclick={onClose}
        >
          <X size={16} strokeWidth={2} aria-hidden="true" />
        </button>
      </div>
      <p class="mt-1 text-[0.866667rem] leading-5 text-muted-foreground">
        {t("vaultHandoff.qrInstructions")}
      </p>
    </div>

    <div class="mt-5">
      {#if pendingEnrollment}
        <EnrollmentApproval pending={pendingEnrollment} busy={approvalBusy} onRespond={onRespondEnrollment} />
      {:else if invitation}
        <PairingQrCode {invitation} onExpired={onRefresh} />
      {:else if invitationError}
        <div class="grid justify-items-center gap-3">
          <div class="flex aspect-square w-full max-w-80">
            <Notice layout="fill" class="text-[0.8rem]" message={invitationError} {onRetry} />
          </div>
          <div class="h-5" aria-hidden="true"></div>
        </div>
      {:else}
        <div class="grid justify-items-center gap-3" role="status" aria-label={t("common.loading")}>
          <div class="grid aspect-square w-full max-w-80 place-items-center bg-white">
            <LoaderCircle size={28} strokeWidth={1.8} class="animate-spin text-black/45" aria-hidden="true" />
          </div>
          <div class="h-5" aria-hidden="true"></div>
        </div>
      {/if}
    </div>

    {#if approvalError}
      <Notice class="mt-3 justify-center text-[0.8rem] leading-5" message={approvalError} />
    {/if}
    <div class="mx-auto mt-3 min-h-9">
      {#if invitation && !pendingEnrollment}
        <button
          type="button"
          class="inline-flex min-h-9 items-center justify-center gap-1.5 rounded-md border border-border px-3 text-[0.8rem] font-medium hover:bg-accent"
          onclick={() => void copyCode()}
        >
          <Copy size={14} strokeWidth={1.8} aria-hidden="true" />
          {copied ? t("vaultHandoff.codeCopied") : t("vaultHandoff.copyCode")}
        </button>
      {/if}
    </div>
    {#if copyFailed}
      <Notice class="mt-2 justify-center text-[0.8rem]" copy={false} message={t("vaultHandoff.copyCodeFailed")} />
    {/if}

    {#if networkAccess?.state === "authorizationRequired" || networkAccess?.state === "networkConfirmationRequired"}
      <div class="mt-4 space-y-2 text-center">
        <p class="text-[0.8rem] leading-5 text-muted-foreground">
          {t("vaultHandoff.networkAccessDescription")}
        </p>
        <button
          type="button"
          class="inline-flex min-h-9 items-center justify-center gap-1.5 rounded-md bg-primary px-3 text-[0.8rem] font-medium text-primary-foreground hover:bg-primary/90 disabled:pointer-events-none disabled:opacity-55"
          disabled={networkBusy}
          onclick={onGrantNetworkAccess}
        >
          {#if networkBusy}<LoaderCircle size={14} class="animate-spin" />{/if}
          {t("vaultHandoff.allowNetworkAccess")}
        </button>
      </div>
    {:else if networkAccess?.state === "manualActionRequired"}
      <Notice class="mt-4 justify-center text-[0.8rem] leading-5" tone="warning" copy={false} message={t("vaultHandoff.networkAccessManual")} />
    {/if}
    {#if networkError}
      <Notice class="mt-3 justify-center text-[0.8rem] leading-5" message={networkError} />
    {/if}
  </div>
</div>
