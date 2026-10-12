<script lang="ts">
  import FolderOpen from "@lucide/svelte/icons/folder-open";
  import FolderInput from "@lucide/svelte/icons/folder-input";
  import Folder from "@lucide/svelte/icons/folder";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import Minus from "@lucide/svelte/icons/minus";
  import Square from "@lucide/svelte/icons/square";
  import X from "@lucide/svelte/icons/x";
  import { onMount } from "svelte";
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
  import VaultSetupContent from "./VaultSetupContent.svelte";
  import VaultWelcomeContent from "./VaultWelcomeContent.svelte";
  import { onboardingPrimaryAction } from "$lib/vault/onboarding-primary-action";

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
    ? "Documents/Ganbaru AI Dev"
    : "Documents/Ganbaru AI";
  type SetupError = { raw: unknown; action: VaultErrorAction };

  let defaultLocation = $state<VaultDefaultLocation | null>(null);
  let busy = $state<"default" | "change" | "import" | null>(null);
  let setupError = $state<SetupError | null>(null);
  let isMaximized = $state(true);
  let welcomeComplete = $state(false);
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

{#snippet setupActions()}
  <div class="grid gap-2">
    <button
      use:onboardingPrimaryAction
      type="button"
      onclick={() => void chooseVault("default")}
      disabled={busy !== null}
      class="flex min-h-11 w-full items-center justify-center gap-2 rounded-md bg-primary px-3 py-2 text-sm font-medium text-primary-foreground transition-colors hover:bg-primary/90 disabled:cursor-not-allowed disabled:opacity-60"
    >
      {#if busy === "default"}
        <LoaderCircle size={16} strokeWidth={2} class="animate-spin" />
      {:else}
        <Folder size={16} strokeWidth={1.8} />
      {/if}
      <span>{t("vaultSetup.useDefaultFolder")}</span>
    </button>

    <div class="grid gap-2 min-[560px]:grid-cols-2">
      <button
        type="button"
        onclick={() => void chooseVault("change")}
        disabled={busy !== null}
        class="flex min-h-11 w-full items-center justify-center gap-2 rounded-md border border-border bg-background px-3 py-2 text-sm font-medium text-foreground transition-colors hover:bg-accent disabled:cursor-not-allowed disabled:opacity-60"
      >
        {#if busy === "change"}
          <LoaderCircle size={16} strokeWidth={2} class="animate-spin" />
        {:else}
          <FolderOpen size={16} strokeWidth={1.8} />
        {/if}
        <span>{t("vaultSetup.changeFolder")}</span>
      </button>

      <button
        type="button"
        onclick={() => void chooseVault("import")}
        disabled={busy !== null}
        class="flex min-h-11 w-full items-center justify-center gap-2 rounded-md border border-border bg-background px-3 py-2 text-sm font-medium text-foreground transition-colors hover:bg-accent disabled:cursor-not-allowed disabled:opacity-60"
      >
        {#if busy === "import"}
          <LoaderCircle size={16} strokeWidth={2} class="animate-spin" />
        {:else}
          <FolderInput size={16} strokeWidth={1.8} />
        {/if}
        <span>{t("vaultSetup.importFolder")}</span>
      </button>
    </div>
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
      {#if !welcomeComplete}
        <VaultWelcomeContent onContinue={() => { welcomeComplete = true; }} />
      {:else}
        <VaultSetupContent
          intro={t("vaultSetup.intro")}
          developmentWarning={defaultLocation?.developmentBuild
            ? t("vaultSetup.developmentBuildWarning", defaultLocation.folderName)
            : null}
          location={defaultLocation?.path ?? fallbackDefaultPath}
          actions={setupActions}
          {error}
        />
      {/if}
    </div>
  </div>

  <WindowResizeHandles disabled={isMaximized} />
</main>
