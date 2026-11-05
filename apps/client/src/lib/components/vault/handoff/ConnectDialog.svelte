<script lang="ts">
  import Check from "@lucide/svelte/icons/check";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import Copy from "@lucide/svelte/icons/copy";
  import ExternalLink from "@lucide/svelte/icons/external-link";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { onMount, tick } from "svelte";
  import { addSyncServer, SyncServerError } from "$lib/api/sync";
  import {
    joinVaultThroughServer,
    recoverVaultThroughServer,
    releaseStarterVault,
  } from "$lib/api/vault-handoff";
  import { formatNumber } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import {
    activateModalFocus,
    activateModalKeyboardLayer,
    trapModalTabKey,
  } from "$lib/modal-focus";
  import { cn } from "$lib/utils";
  import { SERVER_GUIDE_URL, syncServerErrorLabel } from "$lib/sync/server";
  import { writeTextToClipboard } from "$lib/utils/clipboard";
  import { growThenFade } from "$lib/utils/grow-then-fade";
  import { isServerWithoutVault, serverConnectError } from "$lib/vault/server-connect";

  import MaskedCodeInput from "$lib/components/ui/MaskedCodeInput.svelte";
  import EnrollmentCodeWaiting from "./EnrollmentCodeWaiting.svelte";

  /**
   * Connects this computer to a vault it does not have yet: through another computer's pairing
   * code, or through the setup code an anchor server printed. The server path may add the recovery
   * kit, which leaves its field when it is submitted and is kept nowhere else. When
   * `startOnEmptyServer` is set, a server that runs no vault yet receives the active vault and
   * becomes its anchor instead.
   */
  let {
    busy,
    enrollmentCode,
    error,
    startOnEmptyServer = false,
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
    /** Whether an empty server receives the active vault, as onboarding does with the vault it chose. */
    startOnEmptyServer?: boolean;
    onSubmit: (code: string) => void;
    onEdit: () => void;
    onClose: () => void;
    /** Called once the vault copied from the server is the active vault. */
    onServerActivated: () => void;
  } = $props();

  type Source = "computer" | "server";

  /** The command an anchor server operator runs to print a setup code. */
  const SERVER_SETUP_COMMAND = "ganbaru-server anchor setup";
  /** How long the copy button shows that the command was copied. */
  const COPY_FEEDBACK_MS = 2_000;
  /** How long switching sources, the kit field, and messages take, so the dialog never jumps in height. */
  const PANEL_MOTION_MS = 260;

  const localization = getLocalization();
  const { t } = localization;
  const sources: { id: Source; label: () => string }[] = [
    { id: "computer", label: () => t("vaultHandoff.connectTabs.computer") },
    { id: "server", label: () => t("vaultHandoff.connectTabs.server") },
  ];
  let dialog = $state<HTMLDivElement | null>(null);
  let pairingInput = $state<HTMLInputElement | null>(null);
  let serverInput = $state<HTMLInputElement | null>(null);
  let kitInput = $state<HTMLInputElement | null>(null);
  let source = $state<Source>("computer");
  let code = $state("");
  let serverCode = $state("");
  let kit = $state("");
  let useKit = $state(false);
  let serverBusy = $state(false);
  let serverError = $state<string | null>(null);
  let serverNotice = $state<string | null>(null);
  let commandCopied = $state(false);
  let copyFeedbackTimer: number | undefined;
  const panelMotion = { duration: prefersReducedMotion() ? 0 : PANEL_MOTION_MS };
  const anyBusy = $derived(busy || serverBusy);
  const canSubmit = $derived(
    !anyBusy
      && (source === "computer"
        ? code.trim() !== ""
        : serverCode.trim() !== "" && (!useKit || kit.trim() !== "")),
  );

  function close(): void {
    if (!serverBusy) onClose();
  }

  /** Shows `next`, focusing its tab when chosen from the keyboard and its code field otherwise. */
  async function choose(next: Source, focusTab = false): Promise<void> {
    if (anyBusy) return;
    source = next;
    await tick();
    if (focusTab) dialog?.querySelector<HTMLElement>(`#connect-tab-${next}`)?.focus();
    else (next === "computer" ? pairingInput : serverInput)?.focus();
  }

  /**
   * Moves between the two sources with the arrow, Home, and End keys while a source tab has focus,
   * as a tab list does, and answers whether it claimed the key.
   */
  function handleTabKeydown(event: KeyboardEvent): boolean {
    if (!(event.target instanceof HTMLElement) || event.target.getAttribute("role") !== "tab") return false;
    const current = sources.findIndex((entry) => entry.id === source);
    const last = sources.length - 1;
    const target = event.key === "ArrowRight" || event.key === "ArrowDown"
      ? (current + 1) % sources.length
      : event.key === "ArrowLeft" || event.key === "ArrowUp"
        ? (current + last) % sources.length
        : event.key === "Home" ? 0 : event.key === "End" ? last : null;
    if (target === null) return false;
    event.preventDefault();
    void choose(sources[target].id, true);
    return true;
  }

  async function openServerGuide(): Promise<void> {
    try {
      await openUrl(SERVER_GUIDE_URL);
    } catch (cause: unknown) {
      console.warn("server setup guide could not be opened", cause);
    }
  }

  function stepNumber(value: number): string {
    return formatNumber(localization.locale, value);
  }

  function prefersReducedMotion(): boolean {
    return typeof window.matchMedia === "function" && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  }

  async function copyCommand(): Promise<void> {
    try {
      await writeTextToClipboard(SERVER_SETUP_COMMAND);
    } catch (cause: unknown) {
      console.warn("the server setup command could not be copied", cause);
      return;
    }
    commandCopied = true;
    window.clearTimeout(copyFeedbackTimer);
    copyFeedbackTimer = window.setTimeout(() => { commandCopied = false; }, COPY_FEEDBACK_MS);
  }

  async function showKit(): Promise<void> {
    useKit = true;
    serverError = null;
    await tick();
    kitInput?.focus();
  }

  async function connectServer(): Promise<void> {
    const typedCode = serverCode.trim();
    const typedKit = useKit ? kit.trim() : "";
    kit = "";
    serverBusy = true;
    serverError = null;
    serverNotice = null;
    try {
      const outcome = typedKit
        ? await recoverVaultThroughServer(typedCode, typedKit)
        : await joinVaultThroughServer(typedCode);
      if (outcome.inProgress) {
        serverNotice = t("vaultHandoff.copyInProgress");
        return;
      }
      serverCode = "";
      if (outcome.activated) onServerActivated();
    } catch (cause) {
      if (startOnEmptyServer && !typedKit && isServerWithoutVault(cause)) {
        await startOnServer(typedCode);
      } else {
        serverError = serverConnectError(cause, t);
      }
    } finally {
      serverBusy = false;
    }
  }

  /**
   * Puts the active vault on a server that runs none yet and makes the server its anchor. The
   * vault becomes user data first, so a later link keeps a backup of it.
   */
  async function startOnServer(typedCode: string): Promise<void> {
    try {
      await releaseStarterVault();
      const step = await addSyncServer(typedCode, true);
      if (step === "waitingForAnchor") {
        serverNotice = t("sync.server.anchorWaiting");
        return;
      }
      serverCode = "";
      onServerActivated();
    } catch (cause) {
      console.warn("the vault could not be added to the server", cause);
      serverError = cause instanceof SyncServerError
        ? syncServerErrorLabel(t, cause)
        : t("vaultHandoff.serverConnect.errors.failed");
    }
  }

  function submit(): void {
    if (!canSubmit) return;
    if (source === "computer") onSubmit(code.trim());
    else void connectServer();
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (event.key === "Tab" && dialog) {
      trapModalTabKey(dialog, event);
      event.stopPropagation();
      return;
    }
    if (handleTabKeydown(event)) {
      event.stopPropagation();
      return;
    }
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      close();
      return;
    }
    if (event.key === "Enter" && event.target instanceof HTMLInputElement) {
      event.preventDefault();
      submit();
    }
    event.stopPropagation();
  }

  onMount(() => {
    const deactivateKeyboard = activateModalKeyboardLayer(handleKeydown);
    let deactivateFocus = (): void => undefined;
    void tick().then(() => {
      if (dialog) deactivateFocus = activateModalFocus(dialog, pairingInput);
    });
    return () => {
      deactivateKeyboard();
      deactivateFocus();
      window.clearTimeout(copyFeedbackTimer);
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
    <h2 id="connect-dialog-title" class="text-lg leading-8 font-semibold text-foreground">
      {t("vaultHandoff.connectTitle")}
      <span class="inline-flex flex-wrap items-baseline gap-x-1.5" role="tablist" aria-orientation="horizontal">
        {#each sources as entry, index (entry.id)}
          {@const active = source === entry.id}
          {#if index > 0}<span class="font-normal text-muted-foreground">{t("vaultHandoff.connectOr")}</span>{/if}
          <button
            type="button"
            role="tab"
            id={`connect-tab-${entry.id}`}
            aria-selected={active}
            aria-controls="connect-dialog-panel"
            tabindex={active ? 0 : -1}
            class={cn(
              "rounded-sm whitespace-nowrap underline decoration-2 underline-offset-6 transition-colors disabled:pointer-events-none",
              active
                ? "text-foreground decoration-primary"
                : "cursor-pointer text-muted-foreground decoration-muted-foreground/40 decoration-dotted hover:text-foreground hover:decoration-foreground/60",
            )}
            disabled={anyBusy}
            onclick={() => void choose(entry.id)}
          >
            {entry.label()}
          </button>
        {/each}
      </span>
    </h2>

    <!-- The marker column spans its whole row, so each line runs from one circle to the next. Next to a field, the circle drops to the field's center. -->
    {#snippet step(number: number, total: number, beside: "text" | "field")}
      <span aria-hidden="true" class="flex w-6 shrink-0 flex-col items-center">
        <span class={cn("w-px shrink-0", beside === "field" ? "h-1" : "h-0", number > 1 && "bg-primary/25")}></span>
        <span class="flex size-6 shrink-0 items-center justify-center rounded-full bg-primary/12 text-xs font-semibold text-primary tabular-nums">
          {stepNumber(number)}
        </span>
        <span class={cn("w-px flex-1", number < total && "bg-primary/25")}></span>
      </span>
    {/snippet}

    <!-- Both sources share one cell, so switching grows or shrinks the dialog once while the content fades. -->
    <div id="connect-dialog-panel" role="tabpanel" aria-labelledby={`connect-tab-${source}`} class="mt-5 grid items-start">
      {#if source === "computer"}
        <div transition:growThenFade={panelMotion} class="col-start-1 row-start-1 min-w-0">
          <ol class="flex flex-col">
            <li class="flex gap-3">
              {@render step(1, 3, "text")}
              <p class="min-w-0 pt-0.5 pb-4 text-sm leading-5 text-foreground">
                {t("vaultHandoff.computerSteps.open")}
                <span class="ms-0.5 inline-flex items-center gap-0.5 rounded-md bg-muted px-1.5 py-px align-baseline text-[0.8rem] font-medium whitespace-nowrap">
                  {t("settings.title")}<ChevronRight size={12} class="text-muted-foreground rtl:-scale-x-100" aria-hidden="true" />{t("settings.section.data")}
                </span>
              </p>
            </li>
            <li class="flex gap-3">
              {@render step(2, 3, "text")}
              <p class="min-w-0 pt-0.5 pb-4 text-sm leading-5 text-foreground">{t("vaultHandoff.computerSteps.copy")}</p>
            </li>
            <li class="flex gap-3">
              {@render step(3, 3, "field")}
              <div class="min-w-0 flex-1">
                <MaskedCodeInput
                  bind:input={pairingInput}
                  bind:value={code}
                  id="pairing-code"
                  clearLabel={t("vaultHandoff.clearCode")}
                  autocomplete="off"
                  spellcheck="false"
                  class="field h-8 w-full px-3 text-sm"
                  placeholder={t("vaultHandoff.codePlaceholder")}
                  aria-label={t("vaultHandoff.codePlaceholder")}
                  aria-invalid={error !== null}
                  aria-describedby={error ? "pairing-code-error" : undefined}
                  disabled={busy}
                  oninput={onEdit}
                />
                {#if error}
                  <p transition:growThenFade={panelMotion} id="pairing-code-error" role="alert" class="mt-2 text-[0.8rem] leading-5 text-destructive">{error}</p>
                {/if}
              </div>
            </li>
          </ol>
          {#if busy && enrollmentCode}
            <div transition:growThenFade={panelMotion} class="mt-5">
              <EnrollmentCodeWaiting code={enrollmentCode} />
            </div>
          {/if}
        </div>
      {:else}
        <div transition:growThenFade={panelMotion} class="col-start-1 row-start-1 min-w-0">
          <ol class="flex flex-col">
            <li class="flex gap-3">
              {@render step(1, 3, "text")}
              <div class="min-w-0 flex-1 pt-0.5 pb-4">
                <p class="text-sm leading-5 text-foreground">{t("vaultHandoff.serverConnect.install")}</p>
                <button
                  type="button"
                  class="mt-1 inline-flex items-center gap-1 text-[0.8rem] text-muted-foreground underline underline-offset-2 hover:text-foreground"
                  onclick={() => void openServerGuide()}
                >
                  {t("sync.server.guide")}<ExternalLink size={12} aria-hidden="true" />
                </button>
              </div>
            </li>
            <li class="flex gap-3">
              {@render step(2, 3, "text")}
              <div class="min-w-0 flex-1 pt-0.5 pb-4">
                <p class="text-sm leading-5 text-foreground">{t("vaultHandoff.serverConnect.run")}</p>
                <div class="mt-2 flex h-8 items-center gap-2 rounded-md bg-muted ps-3 pe-1" dir="ltr">
                  <code class="min-w-0 flex-1 overflow-x-auto font-mono text-[0.8rem] whitespace-nowrap text-foreground">
                    <span class="text-muted-foreground select-none" aria-hidden="true">$</span> {SERVER_SETUP_COMMAND}
                  </code>
                  <button
                    type="button"
                    data-copy-command
                    class="flex size-6 shrink-0 items-center justify-center rounded-sm text-muted-foreground hover:bg-background hover:text-foreground"
                    title={commandCopied ? t("vaultHandoff.serverConnect.commandCopied") : t("vaultHandoff.serverConnect.copyCommand")}
                    aria-label={commandCopied ? t("vaultHandoff.serverConnect.commandCopied") : t("vaultHandoff.serverConnect.copyCommand")}
                    onclick={() => void copyCommand()}
                  >
                    {#if commandCopied}<Check size={14} aria-hidden="true" />{:else}<Copy size={14} aria-hidden="true" />{/if}
                  </button>
                </div>
              </div>
            </li>
            <li class="flex gap-3">
              {@render step(3, 3, "field")}
              <div class="min-w-0 flex-1">
                <MaskedCodeInput
                  bind:input={serverInput}
                  bind:value={serverCode}
                  id="server-connect-code"
                  clearLabel={t("vaultHandoff.clearCode")}
                  class="field h-8 w-full px-3 font-mono text-sm"
                  dir="ltr"
                  autocomplete="off"
                  autocapitalize="off"
                  spellcheck="false"
                  placeholder="ganbaru-anchor:…"
                  aria-label={t("vaultHandoff.serverConnect.codeLabel")}
                  aria-invalid={serverError !== null}
                  aria-describedby={serverError ? "server-connect-error" : "server-connect-outcome"}
                  disabled={serverBusy}
                  oninput={() => { serverError = null; }}
                />
                <p id="server-connect-outcome" class="mt-2 text-[0.8rem] leading-5 text-muted-foreground">
                  {startOnEmptyServer ? t("vaultHandoff.serverConnect.startOutcome") : t("vaultHandoff.serverConnect.outcome")}
                </p>
              </div>
            </li>
          </ol>
          <!-- The kit field and the prompt that reveals it share one cell, like the two sources. -->
          <div class="grid items-start">
            {#if useKit}
              <div transition:growThenFade={panelMotion} class="col-start-1 row-start-1 mt-4 min-w-0">
                <label class="mb-1 block text-[0.8rem] text-muted-foreground" for="server-connect-kit">{t("vaultHandoff.serverConnect.kitLabel")}</label>
                <MaskedCodeInput
                  bind:input={kitInput}
                  bind:value={kit}
                  id="server-connect-kit"
                  clearLabel={t("vaultHandoff.clearKit")}
                  class="field h-8 w-full px-3 font-mono text-sm"
                  dir="ltr"
                  autocomplete="off"
                  autocapitalize="off"
                  spellcheck="false"
                  disabled={serverBusy}
                  oninput={() => { serverError = null; }}
                />
              </div>
            {:else}
              <p transition:growThenFade={panelMotion} class="col-start-1 row-start-1 mt-4 border-t border-border pt-3 text-[0.8rem] leading-5 text-muted-foreground">
                {t("vaultHandoff.serverConnect.kitPrompt")}
                <button
                  type="button"
                  class="text-foreground underline underline-offset-2 hover:text-primary"
                  disabled={serverBusy}
                  onclick={() => void showKit()}
                >
                  {t("vaultHandoff.serverConnect.useKit")}
                </button>
              </p>
            {/if}
          </div>
          {#if serverError}
            <p transition:growThenFade={panelMotion} id="server-connect-error" role="alert" class="mt-2 text-[0.8rem] leading-5 text-destructive">{serverError}</p>
          {:else if serverNotice}
            <p transition:growThenFade={panelMotion} role="status" class="mt-2 text-[0.8rem] leading-5 text-muted-foreground">{serverNotice}</p>
          {/if}
        </div>
      {/if}
    </div>

    <div class="mt-5 flex justify-end gap-2">
      <button
        type="button"
        class="inline-flex min-h-9 items-center justify-center rounded-md border border-border px-3 text-[0.8rem] font-medium hover:bg-accent"
        disabled={anyBusy}
        onclick={close}
      >
        {t("common.close")}
      </button>
      <button
        type="button"
        data-connect-submit
        class="inline-flex min-h-9 items-center justify-center gap-1.5 rounded-md bg-primary px-3 text-[0.8rem] font-medium text-primary-foreground hover:bg-primary/90 disabled:pointer-events-none disabled:opacity-55"
        disabled={!canSubmit}
        onclick={submit}
      >
        {#if anyBusy}<LoaderCircle size={14} class="animate-spin" />{/if}
        {#if source === "computer"}
          {t("vaultHandoff.linkDevice")}
        {:else}
          {serverBusy ? t("vaultHandoff.serverConnect.connecting") : t("vaultHandoff.serverConnect.action")}
        {/if}
      </button>
    </div>
  </div>
</div>
