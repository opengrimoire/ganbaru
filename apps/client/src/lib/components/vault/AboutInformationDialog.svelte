<script lang="ts">
  import X from "@lucide/svelte/icons/x";
  import { onMount, tick } from "svelte";
  import AboutAcknowledgments from "$lib/components/settings/sections/AboutAcknowledgments.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { activateModalFocus, activateModalKeyboardLayer, trapModalTabKey } from "$lib/modal-focus";
  import { BUILD_PLATFORM_PROFILE, platformHasCapability } from "$lib/platform";
  import { getMobileBackStack } from "$lib/stores/mobile-back-stack.svelte";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";
  import licenseText from "../../../../../../LICENSE?raw";

  let { kind, onClose }: { kind: "license" | "acknowledgments"; onClose: () => void } = $props();
  const { t } = getLocalization();
  const title = $derived(kind === "license" ? t("vaultSetup.licenseButton") : t("settings.about.acknowledgmentsHeading"));
  let dialog = $state<HTMLDivElement | null>(null);
  let content = $state<HTMLDivElement | null>(null);

  function handleKeydown(event: KeyboardEvent): void {
    if (event.key === "Tab" && dialog) trapModalTabKey(dialog, event);
    if (event.key === "Escape") {
      event.preventDefault();
      onClose();
    }
    event.stopPropagation();
  }

  onMount(() => {
    const deactivateBack = platformHasCapability(BUILD_PLATFORM_PROFILE, "system.android-back")
      ? getMobileBackStack().activate({ handle: onClose })
      : () => undefined;
    const deactivateKeyboard = activateModalKeyboardLayer(handleKeydown);
    let deactivateFocus = (): void => undefined;
    void tick().then(() => {
      if (dialog) deactivateFocus = activateModalFocus(dialog, content);
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
<div class="fixed z-90 flex items-center justify-center" style="left: var(--visual-viewport-offset-left); top: var(--visual-viewport-offset-top); width: var(--visual-viewport-width); height: var(--visual-viewport-height); padding: calc(var(--safe-area-top) + 1rem) calc(var(--safe-area-right) + 1rem) calc(var(--safe-area-bottom) + 1rem) calc(var(--safe-area-left) + 1rem);" onclick={onClose}>
  <div class="surface-backdrop absolute inset-0"></div>
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div bind:this={dialog} role="dialog" aria-modal="true" aria-label={title} tabindex="-1" class="surface-dialog relative z-10 flex w-full max-w-2xl flex-col overflow-hidden py-6 pr-2 pl-6 outline-none sm:py-8 sm:pr-3 sm:pl-8" style="max-height: min(80%, 48rem);" onclick={(event) => event.stopPropagation()} onkeydown={handleKeydown}>
    <button type="button" aria-label={t("common.close")} class="absolute top-5 right-5 z-10 flex size-10 items-center justify-center rounded-full bg-card/90 text-muted-foreground transition-colors hover:bg-accent hover:text-foreground focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring sm:top-6" onclick={onClose}>
      <X size={18} strokeWidth={1.8} aria-hidden="true" />
    </button>
    <!-- Focus the reading area so keyboard users can scroll long content. -->
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <div bind:this={content} use:scrollEdgeFadeAction role="document" aria-label={title} tabindex="0" class="min-h-0 overflow-y-auto overscroll-contain pr-4 outline-none sm:pr-5">
      {#if kind === "license"}
        <pre class="wrap-break-word whitespace-pre-wrap text-xs leading-5 text-foreground" data-selectable-content>{licenseText}</pre>
      {:else}
        <AboutAcknowledgments showHeading={false} />
      {/if}
    </div>
  </div>
</div>
