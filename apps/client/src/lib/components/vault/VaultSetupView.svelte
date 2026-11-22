<script lang="ts">
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import { onMount, untrack } from "svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import {
    clearPreVaultLanguagePreference,
    readPreVaultLanguagePreference,
  } from "$lib/i18n/pre-vault-language";
  import { ensureConfigLoaded, flushConfig } from "$lib/vault/config";
  import {
    formatVaultError,
    getDefaultVaultLocation,
    pickOpenVault,
    pickCreateVault,
    useDefaultVault,
    type VaultDefaultLocation,
    type VaultInfo,
  } from "$lib/vault/state";
  import { WELCOME_LINK_CLASS } from "$lib/vault/welcome-layout";
  import { WELCOME_DATA_STEP } from "$lib/vault/welcome-steps";
  import OnboardingWindowFrame from "./OnboardingWindowFrame.svelte";
  import VaultWelcomeContent from "./VaultWelcomeContent.svelte";
  import WelcomeDataStep from "./welcome/WelcomeDataStep.svelte";
  import { NO_VAULT_SETUP_ERRORS, recordVaultSetupFailure, vaultSetupErrorText, type VaultSetupErrors } from "./vault-setup-errors";

  let {
    initialError = null,
    onReady,
  }: {
    initialError?: string | null;
    onReady: (info: VaultInfo, preparation: Promise<void>) => void | Promise<void>;
  } = $props();

  const localization = getLocalization();
  const { t } = localization;
  const fallbackDefaultPath = import.meta.env.DEV
    ? "Documents/Ganbaru Dev"
    : "Documents/Ganbaru";
  let defaultLocation = $state<VaultDefaultLocation | null>(null);
  let busy = $state<"default" | "change" | "import" | null>(null);
  let setupErrors = $state.raw<VaultSetupErrors>(NO_VAULT_SETUP_ERRORS);
  /** A vault that failed to open at startup skips the language screen and lands on the data screen with the error. */
  let step = $state(untrack(() => (initialError ? WELCOME_DATA_STEP : 0)));
  const error = $derived(
    vaultSetupErrorText(setupErrors, (failure) => formatVaultError(failure.raw, failure.action, t)),
  );

  function safeStorage(): Storage | undefined {
    if (typeof window === "undefined") return undefined;
    try {
      return window.localStorage;
    } catch {
      return undefined;
    }
  }

  onMount(() => {
    if (initialError) {
      setupErrors = recordVaultSetupFailure(setupErrors, "attempt", { raw: initialError, action: "startup" });
    }
    void loadDefaultLocation();
  });

  async function loadDefaultLocation(): Promise<void> {
    try {
      defaultLocation = await getDefaultVaultLocation();
    } catch (err) {
      console.warn("Could not load the default vault location:", err);
      setupErrors = recordVaultSetupFailure(setupErrors, "location", { raw: err, action: "general" });
    }
  }

  async function persistPreVaultLanguagePreference(): Promise<void> {
    const storage = safeStorage();
    const preference = readPreVaultLanguagePreference(storage);
    if (!preference) return;
    try {
      await ensureConfigLoaded();
      const applied = await localization.setLanguagePreference(preference, { persist: true });
      if (!applied) return;
      await flushConfig();
      clearPreVaultLanguagePreference(storage);
    } catch (err) {
      console.warn("pre-vault language preference could not be saved", err);
    }
  }

  async function chooseVault(mode: "default" | "change" | "import"): Promise<void> {
    if (busy) return;
    busy = mode;
    try {
      const info =
        mode === "default"
          ? await useDefaultVault()
          : mode === "change"
            ? await pickCreateVault()
            : await pickOpenVault();
      if (info) {
        const preparation = persistPreVaultLanguagePreference();
        void preparation.catch(() => undefined);
        await onReady(info, preparation);
      }
    } catch (err) {
      console.warn(`Could not ${mode} the vault:`, err);
      setupErrors = recordVaultSetupFailure(setupErrors, "attempt", { raw: err, action: mode });
    } finally {
      busy = null;
    }
  }
</script>

{#snippet dataStep()}
  <WelcomeDataStep
    headline={t("vaultSetup.title")}
    location={defaultLocation?.path ?? fallbackDefaultPath}
    locationLabel={t("vaultSetup.defaultLocation")}
    places={defaultLocation?.places}
    {error}
    choices={dataChoices}
  />
{/snippet}

{#snippet dataChoices()}
  <button type="button" onclick={() => void chooseVault("change")} disabled={busy !== null} class={WELCOME_LINK_CLASS}>
    {t("vaultSetup.changeFolder")}
    {#if busy === "change"}<LoaderCircle size={14} strokeWidth={2} class="animate-spin" aria-hidden="true" />{/if}
  </button>
  <button type="button" onclick={() => void chooseVault("import")} disabled={busy !== null} class={WELCOME_LINK_CLASS}>
    {t("vaultSetup.importFolder")}
    {#if busy === "import"}<LoaderCircle size={14} strokeWidth={2} class="animate-spin" aria-hidden="true" />{/if}
  </button>
{/snippet}

<OnboardingWindowFrame class="setup-shell app-shell">
  <VaultWelcomeContent
    stage="setup"
    bind:step
    data={dataStep}
    last={{
      label: t("vaultSetup.useDefaultFolder"),
      onClick: () => void chooseVault("default"),
      busy: busy === "default",
      disabled: busy !== null,
    }}
    lastBackDisabled={busy !== null}
  />
</OnboardingWindowFrame>
