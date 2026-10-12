<script lang="ts">
  import { onMount, tick } from "svelte";
  import Command from "@lucide/svelte/icons/command";
  import MessageSquarePlus from "@lucide/svelte/icons/message-square-plus";
  import PanelBottom from "@lucide/svelte/icons/panel-bottom";
  import PanelRight from "@lucide/svelte/icons/panel-right";
  import Search from "@lucide/svelte/icons/search";
  import Settings from "@lucide/svelte/icons/settings";
  import { firstFocusable, trapTabFocus } from "$lib/chat/focus-navigation";
  import { getLocalization } from "$lib/i18n/translator.svelte";

  let {
    bottomPanelOpen,
    onClose,
    onNewChannel,
    onSearch,
    onToggleBottomPanel,
    onToggleInspector,
    onOpenSettings,
  }: {
    bottomPanelOpen: boolean;
    onClose: () => void;
    onNewChannel: () => void;
    onSearch: () => void;
    onToggleBottomPanel: () => void;
    onToggleInspector: () => void;
    onOpenSettings: () => void;
  } = $props();

  const { t } = getLocalization();
  let dialog: HTMLDivElement | undefined = $state();
  let returnFocus: HTMLElement | null = null;
  let restoreFocusOnDestroy = true;

  onMount(() => {
    returnFocus = document.activeElement instanceof HTMLElement
      ? document.activeElement
      : null;
    void tick().then(() => firstFocusable(dialog)?.focus());
    return () => {
      if (restoreFocusOnDestroy) {
        queueMicrotask(() => returnFocus?.isConnected && returnFocus.focus());
      }
    };
  });

  function run(action: () => void): void {
    onClose();
    action();
  }

  async function openSettings(): Promise<void> {
    restoreFocusOnDestroy = false;
    onClose();
    await tick();
    if (returnFocus?.isConnected) returnFocus.focus();
    onOpenSettings();
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (event.key === "Escape") {
      event.preventDefault();
      onClose();
      return;
    }
    trapTabFocus(event, dialog);
  }
</script>

<div class="absolute inset-0 z-50 flex items-start justify-center surface-backdrop p-3 pt-[10vh]">
  <button type="button" class="absolute inset-0" aria-label={t("chat.commandMenu.close")} onclick={onClose}></button>
  <div bind:this={dialog} class="relative w-full max-w-md surface-floating surface-floating-body" role="dialog" aria-modal="true" aria-label={t("chat.commandMenu.title")} tabindex="-1" onkeydown={handleKeydown}>
    <div class="menu-label flex items-center gap-2"><Command size={14} />{t("chat.commandMenu.title")}</div>
    <button type="button" class="chat-command menu-item" onclick={() => run(onNewChannel)}><MessageSquarePlus size={14} />{t("chat.channels.createTitle")}</button>
    <button type="button" class="chat-command menu-item" onclick={() => run(onSearch)}><Search size={14} />{t("chat.search")}</button>
    <button type="button" class="chat-command menu-item" onclick={() => run(onToggleBottomPanel)}><PanelBottom size={14} />{bottomPanelOpen ? t("chat.closeBottomPanel") : t("chat.openBottomPanel")}</button>
    <button type="button" class="chat-command menu-item" onclick={() => run(onToggleInspector)}><PanelRight size={14} />{t("chat.openInspector")}</button>
    <button type="button" class="chat-command menu-item" onclick={() => void openSettings()}><Settings size={14} />{t("chat.settings")}</button>
  </div>
</div>
