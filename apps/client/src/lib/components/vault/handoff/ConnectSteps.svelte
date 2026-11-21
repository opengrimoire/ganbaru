<script module lang="ts">
  /** Where this device gets a vault it does not have yet. */
  export type ConnectSource = "computer" | "server";
</script>

<script lang="ts">
  import ExternalLink from "@lucide/svelte/icons/external-link";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { tick, untrack, type Snippet } from "svelte";
  import type { HTMLAttributes } from "svelte/elements";
  import { fade } from "svelte/transition";
  import { addSyncServer, SyncServerError } from "$lib/api/sync";
  import {
    decodeServerSetupQr,
    joinVaultThroughServer,
    recoverVaultThroughServer,
    releaseStarterVault,
  } from "$lib/api/vault-handoff";
  import CommandBox from "$lib/components/ui/CommandBox.svelte";
  import MaskedCodeInput from "$lib/components/ui/MaskedCodeInput.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { SERVER_GUIDE_URL, syncServerErrorLabel } from "$lib/sync/server";
  import { cn } from "$lib/utils";
  import { smoothHeight, smoothSwapTiming, swapIn, swapOut } from "$lib/utils/smooth-swap";
  import { isServerWithoutVault, serverConnectError } from "$lib/vault/server-connect";
  import ConnectSourceTabs from "./ConnectSourceTabs.svelte";
  import NumberedStep, { stepCenteredBlockClass, stepTextIndentClass } from "$lib/components/ui/NumberedStep.svelte";
  import EnrollmentCodeWaiting from "./EnrollmentCodeWaiting.svelte";
  import MobilePairingScanner from "./MobilePairingScanner.svelte";
  import RecoveryKitDialog from "./RecoveryKitDialog.svelte";
  import SettingsPath from "$lib/components/settings/SettingsPath.svelte";

  /**
   * The steps that connect this device to a vault it does not have yet: through another
   * computer's pairing code, or through the setup code an anchor server printed. On Android each
   * code can be scanned from its QR code or pasted. The server path may add the recovery kit,
   * which leaves its field when it is submitted and is kept nowhere else. When
   * `startOnEmptyServer` is set, a server that runs no vault yet receives the active vault and
   * becomes its anchor instead. A null `source` shows `children` in the same place, so a caller can
   * offer another way to connect and every switch grows or shrinks the panel once.
   */
  let {
    source,
    platform = "desktop",
    autoScan = false,
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
    loading = false,
    actions,
    children,
  }: {
    source: ConnectSource | null;
    platform?: "desktop" | "android";
    /** Whether Android starts with the camera instead of the code field. */
    autoScan?: boolean;
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
    /** Uses a pairing code; a scanner starts again once the returned promise settles. */
    onSubmit: (code: string) => void | Promise<void>;
    onEdit: () => void;
    /** Called once the vault copied from the server is the active vault. */
    onServerActivated: () => void;
    /** Buttons shown before the submit button, such as a dialog's Close. */
    actions?: Snippet;
    /** The panel shown while `source` is null. */
    children?: Snippet;
    /**
     * Whether `children` is still loading. The panel jumps to the height of loaded content instead of easing, so
     * the first view appears complete and only switches between sources animate.
     */
    loading?: boolean;
  } = $props();

  /** The command an anchor server operator runs to print a setup code. */
  const SERVER_SETUP_COMMAND = "ganbaru-server anchor setup";

  const { t } = getLocalization();
  const mobile = untrack(() => platform === "android");
  let pairingInput = $state<HTMLInputElement | null>(null);
  let serverInput = $state<HTMLInputElement | null>(null);
  let serverField = $state<ReturnType<typeof MaskedCodeInput> | null>(null);
  let kitInput = $state<HTMLInputElement | null>(null);
  let code = $state("");
  let serverCode = $state("");
  let kit = $state("");
  let useKit = $state(false);
  /** Whether a phone shows the modal that takes the recovery kit code; a desktop shows a field instead. */
  let kitDialogOpen = $state(false);
  let serverError = $state<string | null>(null);
  let serverNotice = $state<string | null>(null);
  /** Which sources show the camera instead of the code field; only Android scans. */
  const scanning = $state<Record<ConnectSource, boolean>>(
    untrack(() => ({ computer: mobile && autoScan, server: mobile && autoScan })),
  );
  /** Bumped to start a fresh scanner after a scanned pairing code was used. */
  let scanAttempt = $state(0);
  let cameraFailed = $state(false);
  /** Sources, the kit field, and messages fade while the panel eases to its new height, so it never jumps. */
  const swap = smoothSwapTiming();
  /** Whether height changes jump; it stays set for the frame that lays out the content that ended the loading. */
  let snapHeight = untrack(() => loading);

  $effect(() => {
    if (loading) {
      snapHeight = true;
      return;
    }
    if (!snapHeight) return;
    // The loaded content lays out after the next frame starts, and its resize arrives before the second one.
    let frame = requestAnimationFrame(() => {
      frame = requestAnimationFrame(() => { snapHeight = false; });
    });
    return () => cancelAnimationFrame(frame);
  });
  const anyBusy = $derived(busy || serverBusy);
  /** A scanner submits on its own, so the submit button shows only beside a code field. */
  const footerShown = $derived(source !== null && !scanning[source]);
  const canSubmit = $derived(
    !anyBusy
      && (source === "computer"
        ? code.trim() !== ""
        : source === "server" && serverCode.trim() !== "" && (!useKit || kit.trim() !== "")),
  );
  const fieldHeight = mobile ? "h-11" : "h-8";

  /** Focuses the code field of the shown source once it is on screen. */
  export async function focusField(): Promise<void> {
    await tick();
    (source === "computer" ? pairingInput : source === "server" ? serverInput : null)?.focus();
  }

  /** Uses the shown source's code, when it is complete and nothing is running. */
  export function submit(): void {
    if (!canSubmit) return;
    if (source === "computer") void onSubmit(code.trim());
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

  /**
   * Switches a source between the camera and its code field. The field takes focus when it appears,
   * unless the keyboard chose it, which keeps focus on the scan and paste tabs.
   */
  function showScanner(which: ConnectSource, on: boolean, focus = true): void {
    if (anyBusy || scanning[which] === on) return;
    scanning[which] = on;
    cameraFailed = false;
    if (!on && focus) void focusField();
  }

  /** Attributes of the camera or field under the scan and paste tabs, which make it their tab panel on Android. */
  function codeInputPanel(which: ConnectSource): HTMLAttributes<HTMLDivElement> {
    if (!mobile) return {};
    return {
      id: `${which}-code-input`,
      role: "tabpanel",
      "aria-labelledby": `${which}-code-mode-${scanning[which] ? "scan" : "paste"}`,
    };
  }

  function cameraUnavailable(which: ConnectSource, cause: unknown): void {
    console.warn("the camera could not start", cause);
    scanning[which] = false;
    cameraFailed = true;
    void focusField();
  }

  async function scannedPairing(text: string): Promise<void> {
    await onSubmit(text.trim());
    scanAttempt += 1;
  }

  /** Puts a scanned server code in its field, masked, and connects unless the kit is still missing. */
  async function scannedServer(text: string): Promise<void> {
    scanning.server = false;
    await tick();
    serverField?.fill(text);
    if (canSubmit) submit();
    else if (useKit) kitInput?.focus();
  }

  async function showKit(): Promise<void> {
    useKit = true;
    serverError = null;
    await tick();
    kitInput?.focus();
  }

  /** Keeps the kit code from a phone's modal and connects at once when the server code is already in. */
  function kitEntered(value: string): void {
    kit = value;
    useKit = true;
    kitDialogOpen = false;
    serverError = null;
    if (serverCode.trim() !== "") submit();
  }

  function removeKit(): void {
    kit = "";
    useKit = false;
  }

  async function connectServer(): Promise<void> {
    const typedCode = serverCode.trim();
    const typedKit = useKit ? kit.trim() : "";
    kit = "";
    // A phone's kit lives only in its modal, so a failed try asks for it again from the prompt.
    if (mobile) useKit = false;
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
</script>

{#snippet scanner(which: ConnectSource)}
  <div class={cn("mt-1", stepCenteredBlockClass)}>
    <div class="w-full max-w-80">
      {#if which === "computer"}
        {#key scanAttempt}
          <MobilePairingScanner
            disabled={busy}
            onInvitation={scannedPairing}
            onError={(cause) => cameraUnavailable("computer", cause)}
          />
        {/key}
      {:else}
        <MobilePairingScanner
          decode={decodeServerSetupQr}
          label={t("vaultHandoff.serverConnect.scan")}
          disabled={serverBusy}
          onInvitation={scannedServer}
          onError={(cause) => cameraUnavailable("server", cause)}
        />
      {/if}
    </div>
  </div>
{/snippet}

<!-- On Android each code comes from the camera or the field, so the last step names both as tabs, as the title names the sources. -->
{#snippet codeMode(which: ConnectSource)}
  <p class="pt-0.5 pb-2 text-sm leading-5 text-foreground">
    <ConnectSourceTabs
      sources={[
        { id: "scan", label: t("vaultHandoff.codeMode.scan") },
        { id: "paste", label: t("vaultHandoff.codeMode.paste") },
      ]}
      active={scanning[which] ? "scan" : "paste"}
      disabled={anyBusy}
      idPrefix={`${which}-code-mode`}
      panelId={`${which}-code-input`}
      onChoose={(mode, fromKeyboard) => showScanner(which, mode === "scan", !fromKeyboard)}
    />
    {which === "computer" ? t("vaultHandoff.codeMode.pairingCode") : t("vaultHandoff.codeMode.serverCode")}
  </p>
{/snippet}

{#snippet cameraNotice(which: ConnectSource)}
  {#if mobile && cameraFailed && !scanning[which]}
    <p class="mt-2 text-[0.8rem] leading-5 text-muted-foreground">{t("vaultHandoff.cameraFailed")}</p>
  {/if}
{/snippet}

<!-- A switch is one swap: the old content fades, the panel eases to the new height, and the new content fades in. The buttons hang from the panel's bottom edge, so they stay in place between sources and glide with the height instead of jumping; the steps keep room for them (pb-14 matches the footer's h-14). -->
<div class="relative" {@attach smoothHeight(swap, () => snapHeight)}>
  <!-- svelte-ignore a11y_interactive_supports_focus -->
  <div id={panelId} role="tabpanel" aria-labelledby={labelledBy} class={cn("relative", footerShown && "pb-14")} onkeydown={submitOnEnter}>
    {#if source === "computer"}
      <div in:swapIn={swap} out:swapOut={swap} class="mx-auto w-full max-w-md min-w-0">
        <ol class="flex flex-col">
          <!-- A phone may be the first device with Ganbaru, so its steps start with installing the desktop app. -->
          {#if mobile}
            <NumberedStep number={1} total={3}>
              <p class="min-w-0 pt-0.5 pb-4 text-sm leading-5 text-foreground">{t("vaultHandoff.computerSteps.install")}</p>
            </NumberedStep>
            <NumberedStep number={2} total={3}>
              <p class="min-w-0 pt-0.5 pb-4 text-sm leading-5 text-foreground">
                {t("vaultHandoff.computerSteps.goTo")}
                <SettingsPath trail={[t("settings.section.data"), t("vaultHandoff.heading")]} />
              </p>
            </NumberedStep>
          {:else}
            <NumberedStep number={1} total={3}>
              <p class="min-w-0 pt-0.5 pb-4 text-sm leading-5 text-foreground">
                {t("vaultHandoff.computerSteps.open")}
                <SettingsPath trail={[t("settings.section.data")]} />
              </p>
            </NumberedStep>
            <NumberedStep number={2} total={3}>
              <p class="min-w-0 pt-0.5 pb-4 text-sm leading-5 text-foreground">{t("vaultHandoff.computerSteps.copy")}</p>
            </NumberedStep>
          {/if}
          <NumberedStep number={3} total={3} beside={mobile ? "text" : "field"}>
            <div class="min-w-0 flex-1">
              {#if mobile}{@render codeMode("computer")}{/if}
              <div {...codeInputPanel("computer")}>
                {#if scanning.computer}
                  {@render scanner("computer")}
                {:else}
                  <MaskedCodeInput
                    bind:input={pairingInput}
                    bind:value={code}
                    id="pairing-code"
                    clearLabel={t("common.clearCode")}
                    autocomplete="off"
                    spellcheck="false"
                    class={cn("field w-full px-3 text-sm", fieldHeight)}
                    placeholder={t("vaultHandoff.codePlaceholder")}
                    aria-label={t("vaultHandoff.codePlaceholder")}
                    aria-invalid={error !== null}
                    aria-describedby={error ? "pairing-code-error" : undefined}
                    disabled={busy}
                    oninput={onEdit}
                  />
                {/if}
              </div>
              {#if error}
                <p in:swapIn={swap} out:swapOut={swap} id="pairing-code-error" role="alert" class="mt-2 text-[0.8rem] leading-5 text-destructive">{error}</p>
              {/if}
              {@render cameraNotice("computer")}
            </div>
          </NumberedStep>
        </ol>
        {#if busy && enrollmentCode}
          <div in:swapIn={swap} out:swapOut={swap} class="mt-5">
            <EnrollmentCodeWaiting code={enrollmentCode} />
          </div>
        {/if}
      </div>
    {:else if source === "server"}
      <div in:swapIn={swap} out:swapOut={swap} class="mx-auto w-full max-w-md min-w-0">
        <ol class="flex flex-col">
          <NumberedStep number={1} total={3}>
            <p class="min-w-0 flex-1 pt-0.5 pb-4 text-sm leading-5 text-foreground">
              {t("vaultHandoff.serverConnect.installPrefix")}
              <button
                type="button"
                class="inline-flex items-baseline gap-0.5 text-primary underline underline-offset-2 hover:text-primary/80"
                onclick={() => void openServerGuide()}
              >
                {t("vaultHandoff.serverConnect.installLink")}<ExternalLink size={12} class="self-center" aria-hidden="true" />
              </button>
              {t("vaultHandoff.serverConnect.installSuffix")}
            </p>
          </NumberedStep>
          <NumberedStep number={2} total={3}>
            <div class="min-w-0 flex-1 pt-0.5 pb-4">
              <p class="text-sm leading-5 text-foreground">{t("vaultHandoff.serverConnect.run")}</p>
              <CommandBox class="mt-2" command={SERVER_SETUP_COMMAND} />
            </div>
          </NumberedStep>
          <NumberedStep number={3} total={3} beside={mobile ? "text" : "field"}>
            <div class="min-w-0 flex-1">
              {#if mobile}{@render codeMode("server")}{/if}
              <div {...codeInputPanel("server")}>
                {#if scanning.server}
                  {@render scanner("server")}
                {:else}
                  <MaskedCodeInput
                    bind:this={serverField}
                    bind:input={serverInput}
                    bind:value={serverCode}
                    id="server-connect-code"
                    clearLabel={t("common.clearCode")}
                    class={cn("field w-full px-3 font-mono text-sm", fieldHeight)}
                    dir="ltr"
                    autocomplete="off"
                    autocapitalize="off"
                    spellcheck="false"
                    placeholder="ganbaru-anchor:…"
                    aria-label={t("vaultHandoff.serverConnect.codeLabel")}
                    aria-invalid={serverError !== null}
                    aria-describedby={serverError ? "server-connect-error" : mobile ? undefined : "server-connect-outcome"}
                    disabled={serverBusy}
                    oninput={() => { serverError = null; }}
                  />
                {/if}
              </div>
              {#if !mobile}
                <p id="server-connect-outcome" class="mt-2 text-[0.8rem] leading-5 text-muted-foreground">
                  {startOnEmptyServer ? t("vaultHandoff.serverConnect.startOutcome") : t("vaultHandoff.serverConnect.outcome")}
                </p>
              {/if}
              {@render cameraNotice("server")}
            </div>
          </NumberedStep>
        </ol>
        {#if mobile && useKit}
          <p in:swapIn={swap} out:swapOut={swap} role="status" class="mt-4 text-center text-[0.8rem] leading-5 text-muted-foreground">
            {t("vaultHandoff.serverConnect.kitAdded")}
            <button
              type="button"
              class="text-foreground underline underline-offset-2 hover:text-primary"
              disabled={serverBusy}
              onclick={removeKit}
            >
              {t("vaultHandoff.serverConnect.removeKit")}
            </button>
          </p>
        {:else if useKit}
          <div in:swapIn={swap} out:swapOut={swap} class="mt-4 min-w-0">
            <label class="mb-1 block text-[0.8rem] text-muted-foreground" for="server-connect-kit">{t("vaultHandoff.serverConnect.kitLabel")}</label>
            <MaskedCodeInput
              bind:input={kitInput}
              bind:value={kit}
              id="server-connect-kit"
              clearLabel={t("common.clearKit")}
              class={cn("field w-full px-3 font-mono text-sm", fieldHeight)}
              dir="ltr"
              autocomplete="off"
              autocapitalize="off"
              spellcheck="false"
              disabled={serverBusy}
              oninput={() => { serverError = null; }}
            />
          </div>
        {:else}
          <!-- On a phone the prompt centers under the camera or field without a divider; a desktop sets it off below the steps. -->
          <p in:swapIn={swap} out:swapOut={swap} class={cn("text-[0.8rem] leading-5 text-muted-foreground", mobile ? "mt-4 text-center" : "mt-4 border-t border-border pt-3")}>
            {t("vaultHandoff.serverConnect.kitPrompt")}
            <button
              type="button"
              class="text-foreground underline underline-offset-2 hover:text-primary"
              disabled={serverBusy}
              onclick={() => { if (mobile) kitDialogOpen = true; else void showKit(); }}
            >
              {t("vaultHandoff.serverConnect.useKit")}
            </button>
          </p>
        {/if}
        {#if serverError}
          <p in:swapIn={swap} out:swapOut={swap} id="server-connect-error" role="alert" class={cn("mt-2 text-[0.8rem] leading-5 text-destructive", mobile && stepTextIndentClass)}>{serverError}</p>
        {:else if serverNotice}
          <p in:swapIn={swap} out:swapOut={swap} role="status" class={cn("mt-2 text-[0.8rem] leading-5 text-muted-foreground", mobile && stepTextIndentClass)}>{serverNotice}</p>
        {/if}
      </div>
    {:else if children}
      <div in:swapIn={swap} out:swapOut={swap} class="min-w-0">
        {@render children()}
      </div>
    {/if}
  </div>
  {#if footerShown}
    <div
      in:fade={{ delay: swap.fadeInDelayMs, duration: swap.fadeInMs }}
      out:fade={{ duration: swap.fadeOutMs }}
      class="absolute inset-x-0 bottom-0 mx-auto flex h-14 max-w-md items-end justify-end gap-2"
    >
      {@render actions?.()}
      <button
        type="button"
        data-connect-submit
        class="inline-flex min-h-9 items-center justify-center gap-1.5 rounded-md bg-primary px-3 text-[0.8rem] font-medium text-primary-foreground hover:bg-primary/90 disabled:pointer-events-none disabled:opacity-55 pointer-coarse:min-h-11"
        disabled={!canSubmit}
        onclick={submit}
      >
        {#if anyBusy}<LoaderCircle size={14} class="animate-spin" />{/if}
        {anyBusy ? t("vaultHandoff.connectingAction") : t("vaultHandoff.connectAction")}
      </button>
    </div>
  {/if}
</div>
{#if mobile && kitDialogOpen}
  <RecoveryKitDialog
    confirmLabel={serverCode.trim() !== "" ? t("vaultHandoff.connectAction") : t("common.done")}
    onConfirm={kitEntered}
    onCancel={() => { kitDialogOpen = false; }}
  />
{/if}
