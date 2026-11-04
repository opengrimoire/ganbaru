<script lang="ts">
  import FolderInput from "@lucide/svelte/icons/folder-input";
  import ArchiveRestore from "@lucide/svelte/icons/archive-restore";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import { onMount } from "svelte";
  import BackupKitPrompt from "$lib/components/vault/BackupKitPrompt.svelte";
  import VaultSetupContent from "$lib/components/vault/VaultSetupContent.svelte";
  import { createBackupKitPrompt } from "$lib/vault/backup-kit-prompt.svelte";
  import { backupRefusalMessage } from "$lib/vault/backup-restore";
  import VaultWelcomeContent from "$lib/components/vault/VaultWelcomeContent.svelte";
  import { onboardingPrimaryAction } from "$lib/vault/onboarding-primary-action";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import {
    formatVaultError,
    getDefaultVaultLocation,
    pickOpenVault,
    useDefaultVault,
    type BackupRefusal,
    type VaultDefaultLocation,
    type VaultErrorAction,
    type VaultInfo,
  } from "$lib/vault/state";

  let {
    initialError = null,
    onReady,
  }: {
    initialError?: string | null;
    onReady: (info: VaultInfo) => void;
  } = $props();

  type SetupError =
    | { raw: unknown; action: VaultErrorAction }
    | { refusal: BackupRefusal };

  const { t } = getLocalization();
  const fallbackDefaultPath = import.meta.env.DEV
    ? "Android app storage/Ganbaru Dev"
    : "Android app storage/Ganbaru";
  let defaultLocation = $state<VaultDefaultLocation | null>(null);
  let busy = $state<"default" | "restore" | "import" | null>(null);
  let setupError = $state<SetupError | null>(null);
  let welcomeComplete = $state(false);
  const kitPrompt = createBackupKitPrompt();
  const error = $derived(
    !setupError
      ? null
      : "refusal" in setupError
        ? backupRefusalMessage(setupError.refusal, t)
        : formatVaultError(setupError.raw, setupError.action, t),
  );

  onMount(() => {
    setupError = initialError ? { raw: initialError, action: "startup" } : null;
    void loadDefaultLocation();
  });

  async function loadDefaultLocation(): Promise<void> {
    try {
      defaultLocation = await getDefaultVaultLocation();
    } catch (cause) {
      console.warn("Could not load the default vault location:", cause);
      setupError = { raw: cause, action: "general" };
    }
  }

  /**
   * Restores a picked backup, asking for its recovery kit when needed.
   *
   * @returns The restored vault, or `null` after a cancel or a refusal shown as the setup error.
   */
  async function restoreBackup(): Promise<VaultInfo | null> {
    const outcome = await kitPrompt.restore();
    if (outcome.kind === "restored") return outcome.vault;
    if (outcome.kind === "refused") {
      setupError = { refusal: outcome.reason };
    }
    return null;
  }

  async function chooseVault(mode: "default" | "restore" | "import"): Promise<void> {
    if (busy) return;
    busy = mode;
    setupError = null;
    try {
      const info = mode === "default"
        ? await useDefaultVault()
        : mode === "restore"
          ? await restoreBackup()
          : await pickOpenVault();
      if (info) onReady(info);
    } catch (cause) {
      console.warn(`Could not ${mode} the vault:`, cause);
      setupError = { raw: cause, action: mode };
    } finally {
      busy = null;
    }
  }

</script>

{#snippet setupActions()}
  <div class="grid gap-2">
    <button
      use:onboardingPrimaryAction
      type="button"
      onclick={() => void chooseVault("default")}
      disabled={busy !== null}
      class="flex min-h-11 w-full items-center justify-center gap-2 rounded-md bg-primary px-3 py-2 text-sm font-medium text-primary-foreground transition-colors active:bg-primary/90 disabled:cursor-not-allowed disabled:opacity-60"
    >
      {#if busy === "default"}
        <LoaderCircle size={16} strokeWidth={2} class="animate-spin" aria-hidden="true" />
      {/if}
      <span>{t("mobile.vaultSetup.startFromZero")}</span>
    </button>

    <button
      type="button"
      onclick={() => void chooseVault("restore")}
      disabled={busy !== null}
      class="flex min-h-11 w-full items-center justify-center gap-2 rounded-md border border-border bg-background px-3 py-2 text-sm font-medium text-foreground transition-colors active:bg-accent disabled:cursor-not-allowed disabled:opacity-60"
    >
      {#if busy === "restore"}
        <LoaderCircle size={16} strokeWidth={2} class="animate-spin" aria-hidden="true" />
      {:else}
        <ArchiveRestore size={16} strokeWidth={1.8} aria-hidden="true" />
      {/if}
      <span>{t("mobile.vaultSetup.restoreBackupFile")}</span>
    </button>

    <button
      type="button"
      onclick={() => void chooseVault("import")}
      disabled={busy !== null}
      class="flex min-h-11 w-full items-center justify-center gap-2 rounded-md border border-border bg-background px-3 py-2 text-sm font-medium text-foreground transition-colors active:bg-accent disabled:cursor-not-allowed disabled:opacity-60"
    >
      {#if busy === "import"}
        <LoaderCircle size={16} strokeWidth={2} class="animate-spin" aria-hidden="true" />
      {:else}
        <FolderInput size={16} strokeWidth={1.8} aria-hidden="true" />
      {/if}
      <span>{t("mobile.vaultSetup.importExistingFolder")}</span>
    </button>

  </div>
{/snippet}

<main
  class="mobile-viewport-height w-screen overflow-hidden bg-background pb-(--safe-area-bottom) pt-(--safe-area-top) text-foreground"
  style="padding-left: var(--safe-area-left); padding-right: var(--safe-area-right);"
>
  {#if !welcomeComplete}
    <VaultWelcomeContent onContinue={() => { welcomeComplete = true; }} />
  {:else}
    <VaultSetupContent
      title={t("mobile.vaultSetup.title")}
      intro={t("mobile.vaultSetup.intro")}
      developmentWarning={defaultLocation?.developmentBuild
        ? t("vaultSetup.developmentBuildWarning", defaultLocation.folderName)
        : null}
      location={defaultLocation?.path ?? fallbackDefaultPath}
      locationLabel={t("mobile.vaultSetup.dataLocation")}
      actions={setupActions}
      {error}
    />
  {/if}
</main>

{#if kitPrompt.request}
  <BackupKitPrompt
    refusal={kitPrompt.request.refusal}
    onSubmit={(code) => kitPrompt.answer(code)}
    onCancel={() => kitPrompt.answer(null)}
  />
{/if}
