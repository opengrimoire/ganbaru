<script lang="ts">
  import FolderInput from "@lucide/svelte/icons/folder-input";
  import ArchiveRestore from "@lucide/svelte/icons/archive-restore";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import { onMount } from "svelte";
  import VaultSetupContent from "$lib/components/vault/VaultSetupContent.svelte";
  import VaultWelcomeContent from "$lib/components/vault/VaultWelcomeContent.svelte";
  import { onboardingPrimaryAction } from "$lib/vault/onboarding-primary-action";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import {
    formatVaultError,
    getDefaultVaultLocation,
    pickOpenVault,
    restoreVaultBackup,
    useDefaultVault,
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

  type SetupError = { raw: unknown; action: VaultErrorAction };

  const { t } = getLocalization();
  const fallbackDefaultPath = import.meta.env.DEV
    ? "Android app storage/Ganbaru AI Dev"
    : "Android app storage/Ganbaru AI";
  let defaultLocation = $state<VaultDefaultLocation | null>(null);
  let busy = $state<"default" | "restore" | "import" | null>(null);
  let setupError = $state<SetupError | null>(null);
  let welcomeComplete = $state(false);
  const error = $derived(
    setupError ? formatVaultError(setupError.raw, setupError.action, t) : null,
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

  async function chooseVault(mode: "default" | "restore" | "import"): Promise<void> {
    if (busy) return;
    busy = mode;
    setupError = null;
    try {
      const info = mode === "default"
        ? await useDefaultVault()
        : mode === "restore"
          ? await restoreVaultBackup()
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
