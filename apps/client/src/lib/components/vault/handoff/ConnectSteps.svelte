<script module lang="ts">
  /** Where this computer gets a vault it does not have yet. */
  export type ConnectSource = "computer" | "server";
</script>

<script lang="ts">
  import Check from "@lucide/svelte/icons/check";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import Copy from "@lucide/svelte/icons/copy";
  import ExternalLink from "@lucide/svelte/icons/external-link";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { onMount, tick, type Snippet } from "svelte";
  import { addSyncServer, SyncServerError } from "$lib/api/sync";
  import {
    joinVaultThroughServer,
    recoverVaultThroughServer,
    releaseStarterVault,
  } from "$lib/api/vault-handoff";
  import MaskedCodeInput from "$lib/components/ui/MaskedCodeInput.svelte";
  import { formatNumber } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { SERVER_GUIDE_URL, syncServerErrorLabel } from "$lib/sync/server";
  import { cn } from "$lib/utils";
  import { writeTextToClipboard } from "$lib/utils/clipboard";
  import { smoothHeight, smoothSwapTiming, swapIn, swapOut } from "$lib/utils/smooth-swap";
  import { isServerWithoutVault, serverConnectError } from "$lib/vault/server-connect";
  import EnrollmentCodeWaiting from "./EnrollmentCodeWaiting.svelte";

  /**
   * The steps that connect this computer to a vault it does not have yet: through another
   * computer's pairing code, or through the setup code an anchor server printed. The server path
   * may add the recovery kit, which leaves its field when it is submitted and is kept nowhere else.
   * When `startOnEmptyServer` is set, a server that runs no vault yet receives the active vault and
   * becomes its anchor instead. A null `source` shows `children` in the same place, so a caller can
   * offer another way to connect and every switch grows or shrinks the panel once.
   */
  let {
    source,
    busy,
    enrollmentCode,
    error,
    startOnEmptyServer = false,
    serverBusy = $bindable(false),
    panelId,
    labelledBy,
    onSubmit,
    onEdit,
    onServerActivated,
    actions,
    children,
  }: {
    source: ConnectSource | null;
    /** Whether a pairing code is being used, which the caller owns. */
    busy: boolean;
    /** The code to compare with the coordinator while it decides whether to link this device. */
    enrollmentCode: string | null;
    error: string | null;
    /** Whether an empty server receives the active vault, as onboarding does with the vault it chose. */
    startOnEmptyServer?: boolean;
    /** Whether the server is being reached, which keeps the caller from switching away or closing. */
    serverBusy?: boolean;
    /** Id of the tab panel, which the source tabs control. */
    panelId: string;
    /** Id of the tab that labels the panel. */
    labelledBy: string;
    onSubmit: (code: string) => void;
    onEdit: () => void;
    /** Called once the vault copied from the server is the active vault. */
    onServerActivated: () => void;
    /** Buttons shown before the submit button, such as a dialog's Close. */
    actions?: Snippet;
    /** The panel shown while `source` is null. */
    children?: Snippet;
  } = $props();

  /** The command an anchor server operator runs to print a setup code. */
  const SERVER_SETUP_COMMAND = "ganbaru-server anchor setup";
  /** How long the copy button shows that the command was copied. */
  const COPY_FEEDBACK_MS = 2_000;

  const localization = getLocalization();
  const { t } = localization;
  let pairingInput = $state<HTMLInputElement | null>(null);
  let serverInput = $state<HTMLInputElement | null>(null);
  let kitInput = $state<HTMLInputElement | null>(null);
  let code = $state("");
  let serverCode = $state("");
  let kit = $state("");
  let useKit = $state(false);
  let serverError = $state<string | null>(null);
  let serverNotice = $state<string | null>(null);
  let commandCopied = $state(false);
  let copyFeedbackTimer: number | undefined;
  /** Sources, the kit field, and messages fade while the panel eases to its new height, so it never jumps. */
  const swap = smoothSwapTiming();
  const anyBusy = $derived(busy || serverBusy);
  const canSubmit = $derived(
    !anyBusy
      && (source === "computer"
        ? code.trim() !== ""
        : source === "server" && serverCode.trim() !== "" && (!useKit || kit.trim() !== "")),
  );

  /** Focuses the code field of the shown source once it is on screen. */
  export async function focusField(): Promise<void> {
    await tick();
    (source === "computer" ? pairingInput : source === "server" ? serverInput : null)?.focus();
  }

  /** Uses the shown source's code, when it is complete and nothing is running. */
  export function submit(): void {
    if (!canSubmit) return;
    if (source === "computer") onSubmit(code.trim());
    else void connectServer();
  }

  function submitOnEnter(event: KeyboardEvent): void {
    if (event.key !== "Enter" || !(event.target instanceof HTMLInputElement)) return;
    event.preventDefault();
    submit();
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

  onMount(() => () => window.clearTimeout(copyFeedbackTimer));
</script>

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

{#snippet footer(panelSource: ConnectSource)}
  <div class="mt-5 flex justify-end gap-2">
    {@render actions?.()}
    <button
      type="button"
      data-connect-submit
      class="inline-flex min-h-9 items-center justify-center gap-1.5 rounded-md bg-primary px-3 text-[0.8rem] font-medium text-primary-foreground hover:bg-primary/90 disabled:pointer-events-none disabled:opacity-55"
      disabled={!canSubmit}
      onclick={submit}
    >
      {#if anyBusy}<LoaderCircle size={14} class="animate-spin" />{/if}
      {#if panelSource === "computer"}
        {t("vaultHandoff.linkDevice")}
      {:else}
        {serverBusy ? t("vaultHandoff.serverConnect.connecting") : t("vaultHandoff.serverConnect.action")}
      {/if}
    </button>
  </div>
{/snippet}

<!-- Each source carries its own buttons, so a switch is one swap: the old content fades, the panel eases to the new height, and the new content fades in. -->
<div {@attach smoothHeight(swap)}>
  <!-- svelte-ignore a11y_interactive_supports_focus -->
  <div id={panelId} role="tabpanel" aria-labelledby={labelledBy} class="relative" onkeydown={submitOnEnter}>
    {#if source === "computer"}
      <div in:swapIn={swap} out:swapOut={swap} class="mx-auto w-full max-w-md min-w-0">
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
                <p in:swapIn={swap} out:swapOut={swap} id="pairing-code-error" role="alert" class="mt-2 text-[0.8rem] leading-5 text-destructive">{error}</p>
              {/if}
            </div>
          </li>
        </ol>
        {#if busy && enrollmentCode}
          <div in:swapIn={swap} out:swapOut={swap} class="mt-5">
            <EnrollmentCodeWaiting code={enrollmentCode} />
          </div>
        {/if}
        {@render footer("computer")}
      </div>
    {:else if source === "server"}
      <div in:swapIn={swap} out:swapOut={swap} class="mx-auto w-full max-w-md min-w-0">
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
        {#if useKit}
          <div in:swapIn={swap} out:swapOut={swap} class="mt-4 min-w-0">
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
          <p in:swapIn={swap} out:swapOut={swap} class="mt-4 border-t border-border pt-3 text-[0.8rem] leading-5 text-muted-foreground">
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
        {#if serverError}
          <p in:swapIn={swap} out:swapOut={swap} id="server-connect-error" role="alert" class="mt-2 text-[0.8rem] leading-5 text-destructive">{serverError}</p>
        {:else if serverNotice}
          <p in:swapIn={swap} out:swapOut={swap} role="status" class="mt-2 text-[0.8rem] leading-5 text-muted-foreground">{serverNotice}</p>
        {/if}
        {@render footer("server")}
      </div>
    {:else if children}
      <div in:swapIn={swap} out:swapOut={swap} class="min-w-0">
        {@render children()}
      </div>
    {/if}
  </div>
</div>
