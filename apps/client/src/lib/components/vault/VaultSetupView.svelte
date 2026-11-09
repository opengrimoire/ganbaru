<script lang="ts">
  import FolderOpen from "@lucide/svelte/icons/folder-open";
  import FolderInput from "@lucide/svelte/icons/folder-input";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import Minus from "@lucide/svelte/icons/minus";
  import Square from "@lucide/svelte/icons/square";
  import X from "@lucide/svelte/icons/x";
  import { onMount, untrack } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import WindowResizeHandles from "$lib/components/WindowResizeHandles.svelte";
  import { isCloseWindowShortcut } from "$lib/windows/title-bar-shortcuts";
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
    type VaultErrorAction,
    type VaultDefaultLocation,
    type VaultInfo,
  } from "$lib/vault/state";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { ONBOARDING_SECONDARY_BUTTON_CLASS, ONBOARDING_SECONDARY_ROW_CLASS } from "$lib/vault/welcome-layout";
  import { WELCOME_STEPS } from "$lib/vault/welcome-steps";
  import VaultWelcomeContent from "./VaultWelcomeContent.svelte";
  import WelcomeDataStep from "./welcome/WelcomeDataStep.svelte";

  let {
    initialError = null,
    onReady,
  }: {
    initialError?: string | null;
    onReady: (info: VaultInfo, preparation: Promise<void>) => void | Promise<void>;
  } = $props();

  const appWindow = getCurrentWindow();
  const localization = getLocalization();
  const { t } = localization;
  const fallbackDefaultPath = import.meta.env.DEV
    ? "Documents/Ganbaru Dev"
    : "Documents/Ganbaru";
  type SetupError = { raw: unknown; action: VaultErrorAction };

  let defaultLocation = $state<VaultDefaultLocation | null>(null);
  let busy = $state<"default" | "change" | "import" | null>(null);
  let setupError = $state<SetupError | null>(null);
  let isMaximized = $state(true);
  /** A vault that failed to open at startup skips the welcome screens and lands on the data screen with the error. */
  let step = $state(untrack(() => (initialError ? WELCOME_STEPS.length : 0)));
  const error = $derived(
    setupError ? formatVaultError(setupError.raw, setupError.action, t) : null,
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
    let cleanupResize: (() => void) | undefined;
    let disposed = false;
    function handleGlobalShortcut(event: KeyboardEvent): void {
      if (!isCloseWindowShortcut(event)) return;
      event.preventDefault();
      event.stopPropagation();
      event.stopImmediatePropagation();
      requestQuit();
    }

    setupError = initialError ? { raw: initialError, action: "startup" } : null;
    void loadDefaultLocation();
    void appWindow.isMaximized().then((value) => {
      if (!disposed) isMaximized = value;
    });
    void appWindow.onResized(() => {
      if (disposed) return;
      void appWindow.isMaximized().then((value) => {
        if (!disposed) isMaximized = value;
      });
    }).then((unlisten) => {
      if (disposed) {
        unlisten();
        return;
      }
      cleanupResize = unlisten;
    });
    window.addEventListener("keydown", handleGlobalShortcut, { capture: true });

    return () => {
      disposed = true;
      cleanupResize?.();
      window.removeEventListener("keydown", handleGlobalShortcut, { capture: true });
    };
  });

  function requestQuit(): void {
    void invoke("force_quit");
  }

  async function loadDefaultLocation(): Promise<void> {
    try {
      defaultLocation = await getDefaultVaultLocation();
    } catch (err) {
      console.warn("Could not load the default vault location:", err);
      setupError = { raw: err, action: "general" };
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
      setupError = { raw: err, action: mode };
    } finally {
      busy = null;
    }
  }
</script>

{#snippet dataStep()}
  <WelcomeDataStep
    headline={t("vaultSetup.title")}
    body={t("vaultSetup.intro")}
    location={defaultLocation?.path ?? fallbackDefaultPath}
    locationLabel={t("vaultSetup.defaultLocation")}
    developmentWarning={defaultLocation?.developmentBuild
      ? t("vaultSetup.developmentBuildWarning", defaultLocation.folderName)
      : null}
  />
{/snippet}

{#snippet dataChoices()}
  {#if error}
    <Notice message={error} class="pb-1 text-sm" />
  {/if}
  <div class={ONBOARDING_SECONDARY_ROW_CLASS}>
    <button
      type="button"
      onclick={() => void chooseVault("change")}
      disabled={busy !== null}
      class={ONBOARDING_SECONDARY_BUTTON_CLASS}
    >
      {#if busy === "change"}
        <LoaderCircle size={16} strokeWidth={2} class="shrink-0 animate-spin" aria-hidden="true" />
      {:else}
        <FolderOpen size={16} strokeWidth={1.8} class="shrink-0" aria-hidden="true" />
      {/if}
      <span class="truncate">{t("vaultSetup.changeFolder")}</span>
    </button>
    <button
      type="button"
      onclick={() => void chooseVault("import")}
      disabled={busy !== null}
      class={ONBOARDING_SECONDARY_BUTTON_CLASS}
    >
      {#if busy === "import"}
        <LoaderCircle size={16} strokeWidth={2} class="shrink-0 animate-spin" aria-hidden="true" />
      {:else}
        <FolderInput size={16} strokeWidth={1.8} class="shrink-0" aria-hidden="true" />
      {/if}
      <span class="truncate">{t("vaultSetup.importFolder")}</span>
    </button>
  </div>
{/snippet}

<main
  class="setup-shell app-shell relative h-screen w-screen overflow-hidden bg-background text-foreground"
>
  <div class="flex h-full min-h-0 flex-col">
    <header
      data-tauri-drag-region
      class="flex h-(--titlebar-h) shrink-0 items-center justify-end bg-background text-foreground"
    >
      <div data-tauri-drag-region class="min-w-0 flex-1 self-stretch"></div>
      <div class="flex h-full">
        <button
          type="button"
          aria-label={t("common.minimize")}
          onclick={() => void appWindow.minimize()}
          class="flex h-full w-10 items-center justify-center text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
        >
          <Minus size={14} strokeWidth={1.8} />
        </button>
        <button
          type="button"
          aria-label={isMaximized ? t("common.restore") : t("common.maximize")}
          onclick={() => void appWindow.toggleMaximize()}
          class="flex h-full w-10 items-center justify-center text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
        >
          <Square size={12} strokeWidth={1.8} />
        </button>
        <button
          type="button"
          aria-label={t("window.close")}
          onclick={requestQuit}
          class="flex h-full w-10 items-center justify-center text-muted-foreground transition-colors hover:bg-destructive hover:text-destructive-foreground"
        >
          <X size={15} strokeWidth={2} />
        </button>
      </div>
    </header>

    <div class="min-h-0 flex-1 bg-background">
      <VaultWelcomeContent
        bind:step
        data={dataStep}
        dataPrimary={{
          label: t("vaultSetup.useDefaultFolder"),
          onClick: () => void chooseVault("default"),
          busy: busy === "default",
          disabled: busy !== null,
        }}
        dataSecondary={dataChoices}
        dataBackDisabled={busy !== null}
      />
    </div>
  </div>

  <WindowResizeHandles disabled={isMaximized} />
</main>
