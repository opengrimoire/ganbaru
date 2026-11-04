<script lang="ts">
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import { onMount, tick } from "svelte";
  import { addSyncServer, SyncServerError } from "$lib/api/sync";
  import {
    joinVaultThroughServer,
    recoverVaultThroughServer,
    releaseStarterVault,
  } from "$lib/api/vault-handoff";
  import Checkbox from "$lib/components/ui/Checkbox.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import {
    activateModalFocus,
    activateModalKeyboardLayer,
    trapModalTabKey,
  } from "$lib/modal-focus";
  import { cn } from "$lib/utils";
  import { syncServerErrorLabel } from "$lib/sync/server";
  import { isServerWithoutVault, serverConnectError } from "$lib/vault/server-connect";

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

  const { t } = getLocalization();
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
  let acknowledged = $state(false);
  let serverBusy = $state(false);
  let serverError = $state<string | null>(null);
  let serverNotice = $state<string | null>(null);
  const anyBusy = $derived(busy || serverBusy);
  const canSubmit = $derived(
    !anyBusy
      && (source === "computer"
        ? code.trim() !== ""
        : acknowledged && serverCode.trim() !== "" && (!useKit || kit.trim() !== "")),
  );

  function close(): void {
    if (!serverBusy) onClose();
  }

  async function choose(next: Source): Promise<void> {
    if (anyBusy) return;
    source = next;
    await tick();
    (next === "computer" ? pairingInput : serverInput)?.focus();
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
      const step = await addSyncServer(typedCode, acknowledged, true);
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
    <h2 id="connect-dialog-title" class="text-base font-semibold text-foreground">
      {t("vaultHandoff.codeDialogTitle")}
    </h2>
    <div class="mt-3 grid grid-cols-2 gap-1 rounded-md border border-border bg-card p-1 dark:bg-transparent" role="tablist" aria-label={t("vaultHandoff.codeDialogTitle")}>
      {#each sources as entry (entry.id)}
        {@const active = source === entry.id}
        <button
          type="button"
          role="tab"
          id={`connect-tab-${entry.id}`}
          aria-selected={active}
          aria-controls="connect-dialog-panel"
          class={cn(
            "flex min-h-7 items-center justify-center rounded-sm px-3 text-center text-[0.8rem] font-medium text-muted-foreground",
            active && "bg-background text-foreground dark:bg-foreground/5",
          )}
          disabled={anyBusy}
          onclick={() => void choose(entry.id)}
        >
          {entry.label()}
        </button>
      {/each}
    </div>

    <div id="connect-dialog-panel" role="tabpanel" aria-labelledby={`connect-tab-${source}`} class="mt-4">
      {#if source === "computer"}
        <p class="text-[0.866667rem] leading-5 text-muted-foreground">{t("vaultHandoff.codeInstructions")}</p>
        <input
          bind:this={pairingInput}
          bind:value={code}
          type="text"
          autocomplete="off"
          spellcheck="false"
          class="field mt-4 h-10 w-full px-3 text-sm"
          placeholder={t("vaultHandoff.codePlaceholder")}
          aria-label={t("vaultHandoff.codePlaceholder")}
          aria-invalid={error !== null}
          aria-describedby={error ? "pairing-code-error" : undefined}
          disabled={busy}
          oninput={onEdit}
        />
        {#if busy && enrollmentCode}
          <div class="mt-5">
            <EnrollmentCodeWaiting code={enrollmentCode} />
          </div>
        {/if}
        {#if error}
          <p id="pairing-code-error" role="alert" class="mt-2 text-[0.8rem] leading-5 text-destructive">{error}</p>
        {/if}
      {:else}
        <p class="text-[0.866667rem] leading-5 text-muted-foreground">
          {startOnEmptyServer ? t("vaultHandoff.serverConnect.startDescription") : t("vaultHandoff.serverConnect.description")}
        </p>
        <label class="mt-4 block text-[0.8rem] text-muted-foreground" for="server-connect-code">{t("vaultHandoff.serverConnect.codeLabel")}</label>
        <input
          bind:this={serverInput}
          bind:value={serverCode}
          id="server-connect-code"
          type="text"
          class="field mt-1 h-10 w-full px-3 font-mono text-sm"
          dir="ltr"
          autocomplete="off"
          autocapitalize="off"
          spellcheck="false"
          placeholder="ganbaru-anchor:…"
          aria-invalid={serverError !== null}
          aria-describedby={serverError ? "server-connect-error" : undefined}
          disabled={serverBusy}
          oninput={() => { serverError = null; }}
        />
        {#if useKit}
          <label class="mt-3 block text-[0.8rem] text-muted-foreground" for="server-connect-kit">{t("vaultHandoff.serverConnect.kitLabel")}</label>
          <input
            bind:this={kitInput}
            bind:value={kit}
            id="server-connect-kit"
            type="text"
            class="field mt-1 h-10 w-full px-3 font-mono text-sm"
            dir="ltr"
            autocomplete="off"
            autocapitalize="off"
            spellcheck="false"
            disabled={serverBusy}
            oninput={() => { serverError = null; }}
          />
        {:else}
          <button
            type="button"
            class="mt-3 text-[0.8rem] text-muted-foreground underline underline-offset-2 hover:text-foreground"
            disabled={serverBusy}
            onclick={() => void showKit()}
          >
            {t("vaultHandoff.serverConnect.useKit")}
          </button>
          <p class="text-[0.8rem] leading-5 text-muted-foreground">{t("vaultHandoff.serverConnect.kitDescription")}</p>
        {/if}
        <label class="mt-3 flex min-w-0 items-start gap-2 text-[0.8rem] leading-5 text-foreground">
          <Checkbox class="mt-0.5" bind:checked={acknowledged} disabled={serverBusy} />
          <span class="min-w-0">{t("sync.server.unreviewedAnchor")}</span>
        </label>
        {#if serverError}
          <p id="server-connect-error" role="alert" class="mt-2 text-[0.8rem] leading-5 text-destructive">{serverError}</p>
        {:else if serverNotice}
          <p role="status" class="mt-2 text-[0.8rem] leading-5 text-muted-foreground">{serverNotice}</p>
        {/if}
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
