<script lang="ts">
  import type { SafetyMode } from "$lib/chat/contracts";
  import { providerPermissionFileName, providerSupportsPermissionMode } from "$lib/chat/composer/permission-modes";
  import * as chatApi from "$lib/api/chat";
  import ConfirmDialog from "$lib/components/ui/ConfirmDialog.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { chatErrorMessage } from "$lib/chat/error-presentation";
  import { useChat } from "$lib/stores/chat.svelte";
  import { portal } from "$lib/utils/portal";
  import ChatControlMenu, { type ChatControlOption } from "./ChatControlMenu.svelte";

  /**
   * Permission mode of a direct provider thread, read from and written to the shared composer state. Channel
   * posts have no per-post permission choice; their agents follow the channel access.
   */
  let { disabled = false }: { disabled?: boolean } = $props();

  const { t } = getLocalization();
  const chat = useChat();
  let confirmationReturnFocus: HTMLElement | null = null;
  let trustDialogOpen = $state(false);
  let fullAccessTrusted = $state(false);
  let pendingTrustedMode = $state<SafetyMode | null>(null);
  let trustKey = $state("");
  let error = $state<string | null>(null);
  const providerInstanceId = $derived(chat.composer.providerInstanceId);
  const workingFolderId = $derived(chat.composer.workingFolderId);
  const provider = $derived(chat.settings?.providerInstances.find((entry) => entry.configuration.instanceId === providerInstanceId) ?? null);
  const providerFamilyId = $derived(provider?.configuration.familyId ?? null);
  const permissionFileName = $derived(providerPermissionFileName(providerFamilyId));
  const selectedMode = $derived(chat.composer.safetyMode ?? "ask_for_approval");
  const trustTitle = $derived(pendingTrustedMode === "custom" ? t("chat.composer.customPermissionsTitle") : t("chat.composer.fullAccessTitle"));
  const trustDescription = $derived(pendingTrustedMode === "custom"
    ? t("chat.composer.customPermissionsTrustDescription", permissionFileName)
    : t("chat.composer.fullAccessDescription"));
  const modeOptions = $derived<ChatControlOption[]>([
    { value: "ask_for_approval", label: t("chat.hero.askForApproval"), description: t("chat.composer.askForApprovalDescription"), icon: "shield-question-mark", disabled: !providerSupportsPermissionMode(providerFamilyId, "ask_for_approval") },
    { value: "approve_for_me", label: t("chat.hero.approveForMe"), description: t("chat.composer.approveForMeDescription"), icon: "shield-check", disabled: !providerSupportsPermissionMode(providerFamilyId, "approve_for_me") },
    { value: "full_access", label: t("chat.hero.fullAccess"), description: t("chat.composer.fullAccessShortDescription"), icon: "shield-alert", disabled: !providerSupportsPermissionMode(providerFamilyId, "full_access") },
    { value: "custom", label: t("chat.hero.customPermissions", permissionFileName), triggerLabel: t("chat.hero.customPermissionsShort"), description: t("chat.composer.customPermissionsDescription", permissionFileName), icon: "settings", disabled: !providerSupportsPermissionMode(providerFamilyId, "custom") },
  ]);

  $effect(() => {
    const folderId = workingFolderId;
    const providerId = providerInstanceId;
    const key = folderId && providerId ? `${folderId}:${providerId}` : "";
    if (key === trustKey) return;
    trustKey = key;
    fullAccessTrusted = false;
    if (folderId && providerId) {
      void chatApi.hasChatFullAccessTrust(providerId, folderId).then((trusted) => {
        if (trustKey === key) fullAccessTrusted = trusted;
      }).catch(() => undefined);
    }
  });

  $effect(() => {
    const mode = selectedMode;
    if (providerFamilyId && !providerSupportsPermissionMode(providerFamilyId, mode)) setMode("ask_for_approval");
  });

  function setMode(mode: SafetyMode): void {
    chat.setComposerModes(mode, chat.composer.interactionMode);
  }

  function chooseMode(value: SafetyMode | ""): void {
    const mode = value || "ask_for_approval";
    if ((mode === "full_access" || mode === "custom") && !fullAccessTrusted) {
      pendingTrustedMode = mode;
      confirmationReturnFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
      trustDialogOpen = true;
      return;
    }
    setMode(mode);
  }

  async function confirmTrust(): Promise<void> {
    const folderId = workingFolderId;
    const providerId = providerInstanceId;
    if (!folderId || !providerId) return;
    error = null;
    try {
      await chatApi.setChatFullAccessTrust(providerId, folderId, true);
      fullAccessTrusted = true;
      setMode(pendingTrustedMode ?? "full_access");
      closeDialog();
    } catch (cause: unknown) {
      error = chatErrorMessage(cause, t("chatErrors.actions.accessAction"));
    }
  }

  function closeDialog(): void {
    trustDialogOpen = false;
    pendingTrustedMode = null;
    const target = confirmationReturnFocus;
    confirmationReturnFocus = null;
    queueMicrotask(() => target?.isConnected && target.focus());
  }
</script>

<div class:full-access={selectedMode === "full_access"} class="permission-mode-control">
  <ChatControlMenu value={selectedMode} options={modeOptions} ariaLabel={t("chat.hero.safety")} dataField="safety" onChange={(value) => chooseMode(value as SafetyMode | "")} compact minimal {disabled} showTooltip={false} />
</div>

{#if trustDialogOpen}
  <div class="contents" use:portal>
    <ConfirmDialog
      title={trustTitle}
      message={error ? `${trustDescription}\n${error}` : trustDescription}
      confirmLabel={pendingTrustedMode === "custom" ? t("chat.composer.confirmCustomPermissions") : t("chat.composer.confirmFullAccess")}
      cancelLabel={t("chat.cancel")}
      danger={false}
      onConfirm={() => void confirmTrust()}
      onCancel={closeDialog}
    />
  </div>
{/if}

<style>
  .permission-mode-control { min-width: 0; color: var(--muted-foreground); }
  .permission-mode-control :global(.control-trigger) { color: currentColor; }
  .permission-mode-control.full-access :global(.control-trigger),
  .permission-mode-control.full-access :global(.control-trigger:hover),
  .permission-mode-control.full-access :global(.control-trigger[aria-expanded="true"]) { color: color-mix(in srgb, color-mix(in srgb, var(--status-tentative) 55%, var(--destructive)) 72%, var(--foreground)); }
  :global(.dark) .permission-mode-control.full-access :global(.control-trigger),
  :global(.dark) .permission-mode-control.full-access :global(.control-trigger:hover),
  :global(.dark) .permission-mode-control.full-access :global(.control-trigger[aria-expanded="true"]) { color: var(--status-tentative); }
</style>
