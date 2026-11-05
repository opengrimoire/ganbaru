<script lang="ts">
  import { onMount, tick } from "svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import {
    activateModalFocus,
    activateModalKeyboardLayer,
    trapModalTabKey,
  } from "$lib/modal-focus";
  import ConnectSourceTabs from "./ConnectSourceTabs.svelte";
  import ConnectSteps, { type ConnectSource } from "./ConnectSteps.svelte";

  /**
   * Connects this computer to a vault it does not have yet, from Settings: through another
   * computer's pairing code, or through the setup code an anchor server printed (see
   * {@link ConnectSteps}).
   */
  let {
    busy,
    enrollmentCode,
    error,
    onSubmit,
    onEdit,
    onClose,
    onServerActivated,
  }: {
    /** Whether a pairing code is being used, which the panel owns. */
    busy: boolean;
    /** The code to compare with the coordinator while it decides whether to link this device. */
    enrollmentCode: string | null;
    error: string | null;
    onSubmit: (code: string) => void;
    onEdit: () => void;
    onClose: () => void;
    /** Called once the vault copied from the server is the active vault. */
    onServerActivated: () => void;
  } = $props();

  const TAB_ID_PREFIX = "connect-tab";
  const PANEL_ID = "connect-dialog-panel";

  const { t } = getLocalization();
  const sources = $derived<{ id: ConnectSource; label: string }[]>([
    { id: "computer", label: t("vaultHandoff.connectTabs.computer") },
    { id: "server", label: t("vaultHandoff.connectTabs.server") },
  ]);
  let dialog = $state<HTMLDivElement | null>(null);
  let tabs = $state<{ handleKeydown: (event: KeyboardEvent) => boolean } | null>(null);
  let steps = $state<{ submit: () => void; focusField: () => Promise<void> } | null>(null);
  let source = $state<ConnectSource>("computer");
  let serverBusy = $state(false);
  const anyBusy = $derived(busy || serverBusy);

  function close(): void {
    if (!serverBusy) onClose();
  }

  /** Shows `next`, focusing its code field unless the keyboard chose it from the tabs. */
  function choose(next: ConnectSource, fromKeyboard: boolean): void {
    if (anyBusy) return;
    source = next;
    if (!fromKeyboard) void steps?.focusField();
  }

  // The modal layer captures keys before the controls see them, so it routes them here.
  function handleKeydown(event: KeyboardEvent): void {
    event.stopPropagation();
    if (event.key === "Tab" && dialog) {
      trapModalTabKey(dialog, event);
      return;
    }
    if (tabs?.handleKeydown(event)) return;
    if (event.key === "Escape") {
      event.preventDefault();
      close();
      return;
    }
    if (event.key === "Enter" && event.target instanceof HTMLInputElement) {
      event.preventDefault();
      steps?.submit();
    }
  }

  onMount(() => {
    const deactivateKeyboard = activateModalKeyboardLayer(handleKeydown);
    let deactivateFocus = (): void => undefined;
    void tick().then(() => {
      if (dialog) deactivateFocus = activateModalFocus(dialog, dialog.querySelector<HTMLInputElement>("#pairing-code"));
    });
    return () => {
      deactivateKeyboard();
      deactivateFocus();
    };
  });
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="fixed inset-0 z-90 flex items-center justify-center p-4" onclick={close}>
  <div class="surface-backdrop absolute inset-0"></div>
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    bind:this={dialog}
    class="relative z-10 max-h-full w-full max-w-md overflow-y-auto surface-dialog px-5 py-5 outline-none sm:px-8"
    role="dialog"
    aria-modal="true"
    aria-labelledby="connect-dialog-title"
    tabindex="-1"
    onclick={(event) => event.stopPropagation()}
    onkeydown={handleKeydown}
  >
    <h2 id="connect-dialog-title" class="mb-5 text-lg leading-8 font-semibold text-foreground">
      {t("vaultHandoff.connectTitle")}
      <ConnectSourceTabs
        bind:this={tabs}
        {sources}
        active={source}
        disabled={anyBusy}
        idPrefix={TAB_ID_PREFIX}
        panelId={PANEL_ID}
        onChoose={choose}
      />
    </h2>

    <ConnectSteps
      bind:this={steps}
      bind:serverBusy
      {source}
      {busy}
      {enrollmentCode}
      {error}
      panelId={PANEL_ID}
      labelledBy={`${TAB_ID_PREFIX}-${source}`}
      {onSubmit}
      {onEdit}
      {onServerActivated}
    >
      {#snippet actions()}
        <button
          type="button"
          class="inline-flex min-h-9 items-center justify-center rounded-md border border-border px-3 text-[0.8rem] font-medium hover:bg-accent"
          disabled={anyBusy}
          onclick={close}
        >
          {t("common.close")}
        </button>
      {/snippet}
    </ConnectSteps>
  </div>
</div>
