<script lang="ts">
  import Minus from "@lucide/svelte/icons/minus";
  import Square from "@lucide/svelte/icons/square";
  import X from "@lucide/svelte/icons/x";
  import { invoke } from "@tauri-apps/api/core";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { onMount } from "svelte";
  import type { PairingInvitation, PairingStatus } from "$lib/api/vault-handoff";
  import WindowResizeHandles from "$lib/components/WindowResizeHandles.svelte";
  import { isCloseWindowShortcut } from "$lib/windows/title-bar-shortcuts";
  import TooltipHost from "$lib/components/ui/TooltipHost.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import VaultHandoffOnboardingContent from "./VaultHandoffOnboardingContent.svelte";

  let {
    vaultId,
    initialInvitation,
    initialStatus,
    onComplete,
  }: {
    vaultId: string;
    initialInvitation: PairingInvitation | null;
    initialStatus: PairingStatus;
    onComplete: () => void | Promise<void>;
  } = $props();

  const { t } = getLocalization();
  const appWindow = getCurrentWindow();
  let isMaximized = $state(true);

  onMount(() => {
    let disposed = false;
    let cleanupResize: (() => void) | undefined;
    function handleGlobalShortcut(event: KeyboardEvent): void {
      if (!isCloseWindowShortcut(event)) return;
      event.preventDefault();
      event.stopPropagation();
      event.stopImmediatePropagation();
      void invoke("force_quit");
    }

    void appWindow.isMaximized().then((value) => {
      if (!disposed) isMaximized = value;
    });
    void appWindow.onResized(() => {
      if (disposed) return;
      void appWindow.isMaximized().then((value) => {
        if (!disposed) isMaximized = value;
      });
    }).then((unlisten) => {
      if (disposed) unlisten();
      else cleanupResize = unlisten;
    });
    window.addEventListener("keydown", handleGlobalShortcut, { capture: true });
    return () => {
      disposed = true;
      cleanupResize?.();
      window.removeEventListener("keydown", handleGlobalShortcut, { capture: true });
    };
  });
</script>

<main class="relative h-screen w-screen overflow-hidden bg-background text-foreground">
  <div class="flex h-full min-h-0 flex-col">
    <header data-tauri-drag-region class="flex h-(--titlebar-h) shrink-0 items-center justify-end">
      <div data-tauri-drag-region class="min-w-0 flex-1 self-stretch"></div>
      <div class="flex h-full">
        <button type="button" aria-label={t("common.minimize")} onclick={() => void appWindow.minimize()} class="flex h-full w-10 items-center justify-center text-muted-foreground hover:bg-accent hover:text-foreground">
          <Minus size={14} strokeWidth={1.8} />
        </button>
        <button type="button" aria-label={isMaximized ? t("common.restore") : t("common.maximize")} onclick={() => void appWindow.toggleMaximize()} class="flex h-full w-10 items-center justify-center text-muted-foreground hover:bg-accent hover:text-foreground">
          <Square size={12} strokeWidth={1.8} />
        </button>
        <button type="button" aria-label={t("window.close")} onclick={() => void invoke("force_quit")} class="flex h-full w-10 items-center justify-center text-muted-foreground hover:bg-destructive hover:text-destructive-foreground">
          <X size={15} strokeWidth={2} />
        </button>
      </div>
    </header>
    <div class="min-h-0 flex-1 overflow-y-auto">
      <VaultHandoffOnboardingContent
        platform="desktop"
        {vaultId}
        {initialInvitation}
        {initialStatus}
        {onComplete}
      />
    </div>
  </div>
  <TooltipHost />
  <WindowResizeHandles disabled={isMaximized} />
</main>
