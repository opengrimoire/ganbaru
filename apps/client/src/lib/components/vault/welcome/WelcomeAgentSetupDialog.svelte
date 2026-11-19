<script module lang="ts">
  import type { ProviderFamilyMetadataRead, ProviderInstallGuide } from "$lib/chat/contracts";
  import type { ChatProviderSetupTarget } from "$lib/settings/types";
  import type { ProviderInstallSituation } from "$lib/components/settings/agents/provider-install-steps";

  /** What the setup dialog shows: the install and sign-in steps, or the full setup form of Settings. */
  export type WelcomeAgentSetupView =
    | { kind: "guide"; guide: ProviderInstallGuide; situation: ProviderInstallSituation; advanced: ChatProviderSetupTarget }
    | { kind: "advanced"; target: ChatProviderSetupTarget };

  /** One provider family with the name the welcome screen shows for it. */
  export type WelcomeAgentFamily = ProviderFamilyMetadataRead & { displayName: string };
</script>

<script lang="ts">
  import X from "@lucide/svelte/icons/x";
  import { onMount, tick } from "svelte";
  import ProviderInstallSteps from "$lib/components/settings/agents/ProviderInstallSteps.svelte";
  import { loadSettingsDetail } from "$lib/components/settings/detail-registry";
  import Loading from "$lib/components/ui/Loading.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { activateModalFocus, activateModalKeyboardLayer, trapModalTabKey } from "$lib/modal-focus";
  import { BUILD_PLATFORM_PROFILE, platformHasCapability } from "$lib/platform";
  import { getMobileBackStack } from "$lib/stores/mobile-back-stack.svelte";
  import { cn } from "$lib/utils";
  import { createBackdropPress } from "$lib/utils/backdrop-press";
  import { portal } from "$lib/utils/portal";
  import { WELCOME_LINK_CLASS } from "$lib/vault/welcome-layout";

  /**
   * Sets up one AI agent from the welcome tour without leaving it: the install and sign-in steps Settings shows, with a
   * way into the full setup form, or that form directly when the agent needs it.
   */
  let {
    family,
    view = $bindable(),
    onRefresh,
    error,
    onDismissError,
    onClose,
  }: {
    family: WelcomeAgentFamily;
    view: WelcomeAgentSetupView;
    /** Checks the agent again after the person ran the steps; the caller reports failures. */
    onRefresh: () => Promise<void>;
    /** Why the last check failed, shown in the dialog since the screen behind it is covered. */
    error: string | null;
    onDismissError: () => void;
    onClose: () => void;
  } = $props();

  const { t } = getLocalization();
  const backdropPress = createBackdropPress();
  let dialog = $state<HTMLDivElement | null>(null);
  const title = $derived(t("settings.agents.providers.setUp", family.displayName));
  function handleKeydown(event: KeyboardEvent): void {
    if (event.key === "Tab" && dialog) {
      trapModalTabKey(dialog, event);
      return;
    }
    if (event.key !== "Escape" || dialog?.querySelector("[data-app-floating-surface]")) return;
    event.preventDefault();
    onClose();
  }

  onMount(() => {
    const deactivateBack = platformHasCapability(BUILD_PLATFORM_PROFILE, "system.android-back")
      ? getMobileBackStack().activate({ handle: onClose })
      : () => undefined;
    const deactivateKeyboard = activateModalKeyboardLayer(handleKeydown);
    let deactivateFocus = (): void => undefined;
    void tick().then(() => {
      if (dialog) deactivateFocus = activateModalFocus(dialog, null);
    });
    return () => {
      deactivateBack();
      deactivateKeyboard();
      deactivateFocus();
    };
  });
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  use:portal
  class="fixed z-90 flex items-center justify-center"
  style="left: var(--visual-viewport-offset-left); top: var(--visual-viewport-offset-top); width: var(--visual-viewport-width); height: var(--visual-viewport-height); padding: calc(var(--safe-area-top) + 1rem) calc(var(--safe-area-right) + 1rem) calc(var(--safe-area-bottom) + 1rem) calc(var(--safe-area-left) + 1rem);"
  onpointerdown={backdropPress.start}
  onclick={(event) => { event.stopPropagation(); if (backdropPress.isClick(event)) onClose(); }}
>
  <div class="surface-backdrop absolute inset-0"></div>
  <div
    bind:this={dialog}
    class={cn(
      "surface-dialog relative z-10 flex max-h-full w-full flex-col overflow-hidden outline-none",
      view.kind === "advanced" ? "max-w-3xl" : "max-w-xl",
    )}
    style={view.kind === "advanced" ? "--foreground: var(--card-foreground); height: min(100%, 44rem);" : "--foreground: var(--card-foreground);"}
    role="dialog"
    aria-modal="true"
    aria-label={title}
    tabindex="-1"
    data-floating-root
    onclick={(event) => event.stopPropagation()}
  >
    {#if view.kind === "guide"}
      {@const guideView = view}
      <header class="flex items-center justify-between gap-3 px-5 pt-4 pb-1">
        <h2 class="truncate text-[1rem] font-semibold">{title}</h2>
        <button type="button" class="dialog-icon-button" aria-label={t("common.close")} onclick={onClose}><X size={16} /></button>
      </header>
      <div class="flex min-h-0 flex-col gap-4 overflow-y-auto px-5 pt-3 pb-5 text-panel">
        {#if error}<Notice message={error} onDismiss={onDismissError} />{/if}
        <ProviderInstallSteps {family} guide={guideView.guide} situation={guideView.situation} {onRefresh} />
        <button
          type="button"
          class={cn(WELCOME_LINK_CLASS, "self-start")}
          onclick={() => (view = { kind: "advanced", target: guideView.advanced })}
        >{t("settings.agents.providers.setUpAdvanced", family.displayName)}</button>
      </div>
    {:else}
      {@const advancedView = view}
      <button type="button" class="dialog-icon-button absolute top-3 right-3 z-10" aria-label={t("common.close")} onclick={onClose}><X size={16} /></button>
      {#await loadSettingsDetail("chat-provider")}
        <Loading layout="fill" class="min-h-28" />
      {:then loaded}
        {#if loaded.kind === "chat-provider"}
          {@const SetupPanel = loaded.component}
          <div class="min-h-0 flex-1">
            <SetupPanel target={advancedView.target} onCancel={onClose} compactLayout />
          </div>
        {/if}
      {:catch}
        <div class="p-5 pr-12"><Notice message={t("vaultSetup.agents.setupFailed")} /></div>
      {/await}
    {/if}
  </div>
</div>
