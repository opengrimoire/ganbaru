<script lang="ts">
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import ScanLine from "@lucide/svelte/icons/scan-line";
  import { tick } from "svelte";
  import {
    decodeServerSetupQr,
    joinVaultThroughServer,
    recoverVaultThroughServer,
  } from "$lib/api/vault-handoff";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { serverConnectError } from "$lib/vault/server-connect";
  import MaskedCodeInput from "$lib/components/ui/MaskedCodeInput.svelte";
  import MobilePairingScanner from "./MobilePairingScanner.svelte";

  /**
   * Connects this phone to the vault an anchor server runs, from the setup QR code the server
   * shows or its pasted text code. The recovery kit may join the code; it leaves its field when it
   * is submitted and is kept nowhere else. The copy replaces the vault on this phone.
   */
  let {
    onActivated,
    onCancel,
  }: {
    /** Called once the vault copied from the server is the active vault. */
    onActivated: () => void;
    /** Returns to linking with a computer. */
    onCancel: () => void;
  } = $props();

  const { t } = getLocalization();
  let codeInput = $state<HTMLInputElement | null>(null);
  let kitInput = $state<HTMLInputElement | null>(null);
  let scanning = $state(true);
  let scannerVersion = $state(0);
  let code = $state("");
  let kit = $state("");
  let useKit = $state(false);
  let busy = $state(false);
  let error = $state<string | null>(null);
  let notice = $state<string | null>(null);
  const canSubmit = $derived(!busy && code.trim() !== "" && (!useKit || kit.trim() !== ""));

  const buttonClass =
    "inline-flex min-h-11 items-center justify-center gap-1.5 rounded-md border border-border px-3 text-sm font-medium transition-colors active:bg-accent disabled:pointer-events-none disabled:opacity-55";
  const primaryButtonClass = `${buttonClass} border-primary bg-primary text-primary-foreground active:bg-primary/90`;

  async function scanned(text: string): Promise<void> {
    code = text;
    scanning = false;
    error = null;
    await tick();
    if (useKit) kitInput?.focus();
  }

  function scanAgain(): void {
    if (busy) return;
    error = null;
    scannerVersion += 1;
    scanning = true;
  }

  function cameraFailed(cause: unknown): void {
    console.warn("the camera could not start for the server QR code", cause);
    scanning = false;
    void tick().then(() => codeInput?.focus());
  }

  async function showKit(): Promise<void> {
    useKit = true;
    error = null;
    await tick();
    kitInput?.focus();
  }

  async function connect(): Promise<void> {
    if (!canSubmit) return;
    const typedCode = code.trim();
    const typedKit = useKit ? kit.trim() : "";
    kit = "";
    busy = true;
    error = null;
    notice = null;
    try {
      const outcome = typedKit
        ? await recoverVaultThroughServer(typedCode, typedKit)
        : await joinVaultThroughServer(typedCode);
      if (outcome.inProgress) {
        notice = t("vaultHandoff.copyInProgress");
        return;
      }
      code = "";
      if (outcome.activated) onActivated();
    } catch (cause) {
      error = serverConnectError(cause, t);
    } finally {
      busy = false;
    }
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (event.key !== "Enter") return;
    event.preventDefault();
    void connect();
  }
</script>

<div class="flex flex-col gap-4">
  <p class="text-sm leading-6 text-muted-foreground">{t("vaultHandoff.serverConnect.phoneDescription")}</p>

  {#if scanning}
    {#key scannerVersion}
      <MobilePairingScanner
        decode={decodeServerSetupQr}
        label={t("vaultHandoff.serverConnect.scan")}
        onInvitation={scanned}
        onError={cameraFailed}
      />
    {/key}
    <button type="button" class={buttonClass} onclick={() => { scanning = false; void tick().then(() => codeInput?.focus()); }}>
      {t("vaultHandoff.serverConnect.paste")}
    </button>
  {:else}
    <div>
      <label class="mb-1 block text-[0.8rem] text-muted-foreground" for="mobile-server-code">{t("vaultHandoff.serverConnect.codeLabel")}</label>
      <MaskedCodeInput
        bind:input={codeInput}
        bind:value={code}
        id="mobile-server-code"
        clearLabel={t("vaultHandoff.clearCode")}
        class="field h-11 w-full px-3 font-mono text-sm"
        dir="ltr"
        autocomplete="off"
        autocapitalize="off"
        spellcheck="false"
        placeholder="ganbaru-anchor:…"
        aria-invalid={error !== null}
        aria-describedby={error ? "mobile-server-error" : undefined}
        disabled={busy}
        oninput={() => { error = null; }}
        onkeydown={handleKeydown}
      />
    </div>
    <button type="button" class={buttonClass} disabled={busy} onclick={scanAgain}>
      <ScanLine size={14} strokeWidth={2} aria-hidden="true" />
      {t("vaultHandoff.serverConnect.scan")}
    </button>
  {/if}

  {#if useKit}
    <div>
      <label class="mb-1 block text-[0.8rem] text-muted-foreground" for="mobile-server-kit">{t("vaultHandoff.serverConnect.kitLabel")}</label>
      <MaskedCodeInput
        bind:input={kitInput}
        bind:value={kit}
        id="mobile-server-kit"
        clearLabel={t("vaultHandoff.clearKit")}
        class="field h-11 w-full px-3 font-mono text-sm"
        dir="ltr"
        autocomplete="off"
        autocapitalize="off"
        spellcheck="false"
        disabled={busy}
        oninput={() => { error = null; }}
        onkeydown={handleKeydown}
      />
    </div>
  {:else}
    <div>
      <button
        type="button"
        class="min-h-11 text-left text-[0.8rem] text-muted-foreground underline underline-offset-2"
        disabled={busy}
        onclick={() => void showKit()}
      >
        {t("vaultHandoff.serverConnect.useKit")}
      </button>
      <p class="text-[0.8rem] leading-5 text-muted-foreground">{t("vaultHandoff.serverConnect.kitDescription")}</p>
    </div>
  {/if}

  {#if error}
    <p id="mobile-server-error" role="alert" class="text-[0.8rem] leading-5 text-destructive">{error}</p>
  {:else if notice}
    <p role="status" class="text-[0.8rem] leading-5 text-muted-foreground">{notice}</p>
  {/if}

  <div class="flex flex-col gap-2">
    <button type="button" class={primaryButtonClass} disabled={!canSubmit} onclick={() => void connect()}>
      {#if busy}<LoaderCircle size={14} class="animate-spin" />{/if}
      {busy ? t("vaultHandoff.serverConnect.connecting") : t("vaultHandoff.serverConnect.action")}
    </button>
    <button type="button" class={buttonClass} disabled={busy} onclick={onCancel}>
      {t("vaultHandoff.serverConnect.useComputer")}
    </button>
  </div>
</div>
