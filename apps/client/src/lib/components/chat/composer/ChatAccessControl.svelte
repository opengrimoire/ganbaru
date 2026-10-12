<script lang="ts">
  import type { SafetyMode } from "$lib/chat/contracts";
  import { providerPermissionFileName, providerSupportsPermissionMode } from "$lib/chat/composer/permission-modes";
  import * as chatApi from "$lib/api/chat";
  import ConfirmDialog from "$lib/components/ui/ConfirmDialog.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getChat } from "$lib/stores/chat.svelte";
  import { portal } from "$lib/utils/portal";
  import ChatControlMenu, { type ChatControlOption } from "./ChatControlMenu.svelte";

  const { t } = getLocalization();
  const chat = getChat();
  let {
    value,
    providerInstanceId,
    workingFolderId,
    disabled = false,
    onChange,
  }: {
    value?: SafetyMode | null;
    providerInstanceId?: string | null;
    workingFolderId?: string | null;
    disabled?: boolean;
    onChange?: (value: SafetyMode) => void;
  } = $props();
  let confirmationReturnFocus: HTMLElement | null = null;
  let fullAccessDialogOpen = $state(false);
  let fullAccessTrusted = $state(false);
  let pendingTrustedMode = $state<SafetyMode | null>(null);
  let trustKey = $state("");
  let error = $state<string | null>(null);
  const controlled = $derived(value !== undefined);
  const selectedProviderInstanceId = $derived(controlled
    ? providerInstanceId ?? null
    : chat.composer.providerInstanceId);
  const selectedWorkingFolderId = $derived(controlled
    ? workingFolderId ?? null
    : chat.composer.workingFolderId);
  const provider = $derived(chat.settings?.providerInstances.find((entry) => entry.configuration.instanceId === selectedProviderInstanceId) ?? null);
  const providerFamilyId = $derived(provider?.configuration.familyId ?? null);
  const permissionFileName = $derived(providerPermissionFileName(providerFamilyId));
  const selectedSafetyMode = $derived(controlled
    ? value ?? "ask_for_approval"
    : chat.composer.safetyMode ?? "ask_for_approval");
  const trustTitle = $derived(pendingTrustedMode === "custom" ? t("chat.composer.customPermissionsTitle") : t("chat.composer.fullAccessTitle"));
  const trustDescription = $derived(pendingTrustedMode === "custom"
    ? t("chat.composer.customPermissionsTrustDescription", permissionFileName)
    : t("chat.composer.fullAccessDescription"));
  const safetyOptions = $derived<ChatControlOption[]>([
    { value: "ask_for_approval", label: t("chat.hero.askForApproval"), description: t("chat.composer.askForApprovalDescription"), icon: "shield-question-mark", disabled: !providerSupportsPermissionMode(providerFamilyId, "ask_for_approval") },
    { value: "approve_for_me", label: t("chat.hero.approveForMe"), description: t("chat.composer.approveForMeDescription"), icon: "shield-check", disabled: !providerSupportsPermissionMode(providerFamilyId, "approve_for_me") },
    { value: "full_access", label: t("chat.hero.fullAccess"), description: t("chat.composer.fullAccessShortDescription"), icon: "shield-alert", disabled: !providerSupportsPermissionMode(providerFamilyId, "full_access") },
    { value: "custom", label: t("chat.hero.customPermissions", permissionFileName), triggerLabel: t("chat.hero.customPermissionsShort"), description: t("chat.composer.customPermissionsDescription", permissionFileName), icon: "settings", disabled: !providerSupportsPermissionMode(providerFamilyId, "custom") },
  ]);

  $effect(() => {
    const folderId = selectedWorkingFolderId;
    const providerId = selectedProviderInstanceId;
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
    const mode = selectedSafetyMode;
    if (mode && providerFamilyId && !providerSupportsPermissionMode(providerFamilyId, mode)) {
      setSafetyMode("ask_for_approval");
    }
  });

  function setSafetyMode(mode: SafetyMode): void {
    if (controlled) {
      onChange?.(mode);
      return;
    }
    chat.setComposerModes(mode, chat.composer.interactionMode);
  }

  function chooseSafety(value: SafetyMode | ""): void {
    const mode = value || "ask_for_approval";
    if ((mode === "full_access" || mode === "custom") && !fullAccessTrusted) {
      pendingTrustedMode = mode;
      confirmationReturnFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
      fullAccessDialogOpen = true;
      return;
    }
    setSafetyMode(mode);
  }

  async function confirmFullAccess(): Promise<void> {
    const folderId = selectedWorkingFolderId;
    const providerId = selectedProviderInstanceId;
    if (controlled && (!folderId || !providerId)) {
      setSafetyMode(pendingTrustedMode ?? "full_access");
      closeDialog();
      return;
    }
    if (!folderId || !providerId) return;
    error = null;
    try {
      await chatApi.setChatFullAccessTrust(providerId, folderId, true);
      fullAccessTrusted = true;
      setSafetyMode(pendingTrustedMode ?? "full_access");
      closeDialog();
    } catch (cause: unknown) {
      error = cause instanceof Error ? cause.message : String(cause);
    }
  }

  function closeDialog(): void {
    fullAccessDialogOpen = false;
    pendingTrustedMode = null;
    const target = confirmationReturnFocus;
    confirmationReturnFocus = null;
    queueMicrotask(() => target?.isConnected && target.focus());
  }

</script>

<div class:controlled class:full-access={selectedSafetyMode === "full_access"} class="access-control">
  <ChatControlMenu value={selectedSafetyMode} options={safetyOptions} ariaLabel={t("chat.hero.safety")} dataField="safety" onChange={(value) => chooseSafety(value as SafetyMode | "")} compact minimal={!controlled} {disabled} showTooltip={false} />
</div>

{#if fullAccessDialogOpen}
  <div class="contents" use:portal>
    <ConfirmDialog
      title={trustTitle}
      message={error ? `${trustDescription}\n${error}` : trustDescription}
      confirmLabel={pendingTrustedMode === "custom" ? t("chat.composer.confirmCustomPermissions") : t("chat.composer.confirmFullAccess")}
      cancelLabel={t("chat.cancel")}
      danger={false}
      onConfirm={() => void confirmFullAccess()}
      onCancel={closeDialog}
    />
  </div>
{/if}

<style>
  .access-control { min-width: 0; color: var(--muted-foreground); }
  .access-control.controlled { color: var(--foreground); }
  .access-control :global(.control-trigger) { color: currentColor; }
  .access-control.controlled :global(.control-trigger) { height: 2rem; border-radius: 999px; background: color-mix(in srgb, var(--accent) 52%, transparent); padding-inline: 0.55rem; }
  .access-control.controlled :global(.control-trigger > svg:last-child) { margin-left: 0.2rem; color: var(--muted-foreground); }
  .access-control.full-access :global(.control-trigger),
  .access-control.full-access :global(.control-trigger:hover),
  .access-control.full-access :global(.control-trigger[aria-expanded="true"]) { color: color-mix(in srgb, color-mix(in srgb, var(--status-tentative) 55%, var(--destructive)) 72%, var(--foreground)); }
  :global(.dark) .access-control.full-access :global(.control-trigger),
  :global(.dark) .access-control.full-access :global(.control-trigger:hover),
  :global(.dark) .access-control.full-access :global(.control-trigger[aria-expanded="true"]) { color: var(--status-tentative); }
</style>
