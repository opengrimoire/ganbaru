<script lang="ts">
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import { onMount, untrack } from "svelte";
  import BackupKitPrompt from "$lib/components/vault/BackupKitPrompt.svelte";
  import WelcomeDataStep from "$lib/components/vault/welcome/WelcomeDataStep.svelte";
  import { createBackupKitPrompt } from "$lib/vault/backup-kit-prompt.svelte";
  import { backupRefusalMessage } from "$lib/vault/backup-restore";
  import VaultWelcomeContent from "$lib/components/vault/VaultWelcomeContent.svelte";
  import MobileOnboardingProgress from "./MobileOnboardingProgress.svelte";
  import { WELCOME_LINK_CLASS } from "$lib/vault/welcome-layout";
  import { WELCOME_STEPS } from "$lib/vault/welcome-steps";
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
  /** A vault that failed to open at startup skips the welcome screens and lands on the data screen with the error. */
  let step = $state(untrack(() => (initialError ? WELCOME_STEPS.length : 0)));
  const onData = $derived(step >= WELCOME_STEPS.length);
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

  /** Opens a vault and keeps its button busy afterwards, since `onReady` replaces this screen. */
  async function chooseVault(mode: "default" | "restore" | "import"): Promise<void> {
    if (busy) return;
    busy = mode;
    setupError = null;
    let opened = false;
    try {
      const info = mode === "default"
        ? await useDefaultVault()
        : mode === "restore"
          ? await restoreBackup()
          : await pickOpenVault();
      if (info) {
        opened = true;
        onReady(info);
      }
    } catch (cause) {
      console.warn(`Could not ${mode} the vault:`, cause);
      setupError = { raw: cause, action: mode };
    } finally {
      if (!opened) busy = null;
    }
  }

</script>

{#snippet dataStep()}
  <WelcomeDataStep
    headline={t("mobile.vaultSetup.title")}
    location={defaultLocation?.path ?? fallbackDefaultPath}
    locationLabel={t("mobile.vaultSetup.dataLocation")}
    showIcons={!(defaultLocation?.developmentBuild ?? false)}
    {error}
    choices={dataChoices}
  />
{/snippet}

{#snippet dataChoices()}
  <button type="button" onclick={() => void chooseVault("restore")} disabled={busy !== null} class={WELCOME_LINK_CLASS}>
    {t("mobile.vaultSetup.restoreBackupFile")}
    {#if busy === "restore"}<LoaderCircle size={14} strokeWidth={2} class="animate-spin" aria-hidden="true" />{/if}
  </button>
  <button type="button" onclick={() => void chooseVault("import")} disabled={busy !== null} class={WELCOME_LINK_CLASS}>
    {t("mobile.vaultSetup.importExistingFolder")}
    {#if busy === "import"}<LoaderCircle size={14} strokeWidth={2} class="animate-spin" aria-hidden="true" />{/if}
  </button>
{/snippet}

<main
  class="mobile-viewport-height flex w-screen flex-col overflow-hidden bg-background pb-(--safe-area-bottom) pt-(--safe-area-top) text-foreground"
  style="padding-left: var(--safe-area-left); padding-right: var(--safe-area-right);"
>
  <MobileOnboardingProgress stage={onData ? "vault" : "welcome"} screen={onData ? 0 : step}>
    <VaultWelcomeContent
      bind:step
      showStepDots={false}
      data={dataStep}
      dataPrimary={{
        label: t("mobile.vaultSetup.startFromZero"),
        onClick: () => void chooseVault("default"),
        busy: busy === "default",
        disabled: busy !== null,
      }}
      dataBackDisabled={busy !== null}
    />
  </MobileOnboardingProgress>
</main>

{#if kitPrompt.request}
  <BackupKitPrompt
    refusal={kitPrompt.request.refusal}
    onSubmit={(code) => kitPrompt.answer(code)}
    onCancel={() => kitPrompt.answer(null)}
  />
{/if}
