<script lang="ts">
  import { onDestroy, tick } from "svelte";
  import { saveSyncRecoveryKitFile, type SyncPendingRecoveryKit } from "$lib/api/sync";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { formatDateTime } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { syncKitCodeLines, syncKitFileName, syncKitFileText } from "$lib/sync/authority";
  import { portal } from "$lib/utils/portal";

  /**
   * Shows a recovery kit that waits to be saved: its code, a printable page with its QR code, a
   * text file export, and the confirmation that asks for the code's last group. The code reaches
   * paper, a file, or the clipboard only when the user chooses to.
   */
  let {
    kit,
    buttonClass,
    exportable = true,
    onConfirm,
  }: {
    kit: SyncPendingRecoveryKit;
    buttonClass: string;
    /** Whether the printable page and the text file export are offered; false where the platform has neither. */
    exportable?: boolean;
    /** Sends the typed last group; resolves true when the kit was confirmed and forgotten. */
    onConfirm: (lastGroup: string) => Promise<boolean>;
  } = $props();

  /** Class on the root element while the kit page prints, so the print rules show only it. */
  const PRINTING_CLASS = "sync-kit-printing";
  const QUIET_ZONE_MODULES = 4;

  const localization = getLocalization();
  const { t } = localization;
  let busy = $state(false);
  let typed = $state("");
  let message = $state("");
  let failed = $state(false);
  let printedAtMs = $state<number | null>(null);

  const codeLines = $derived(syncKitCodeLines(kit.code));
  const qrSide = $derived(kit.qr.width + QUIET_ZONE_MODULES * 2);

  function stopPrinting(): void {
    document.documentElement.classList.remove(PRINTING_CLASS);
    window.removeEventListener("afterprint", stopPrinting);
    printedAtMs = null;
  }

  onDestroy(stopPrinting);

  async function print(): Promise<void> {
    if (printedAtMs !== null) return;
    printedAtMs = Date.now();
    await tick();
    document.documentElement.classList.add(PRINTING_CLASS);
    window.addEventListener("afterprint", stopPrinting);
    try {
      window.print();
      // A webview without print events returns from the blocking dialog with nothing to wait for.
      if (!("onafterprint" in window)) stopPrinting();
    } catch (cause: unknown) {
      console.warn("recovery kit page could not be printed", cause);
      stopPrinting();
    }
  }

  async function saveFile(): Promise<void> {
    if (busy) return;
    busy = true;
    message = "";
    failed = false;
    try {
      await saveSyncRecoveryKitFile(syncKitFileName(t), syncKitFileText(t, localization.locale, kit.code, Date.now()));
    } catch (cause: unknown) {
      console.warn("recovery kit file could not be saved", cause);
      failed = true;
      message = t("sync.kit.saveFailed");
    } finally {
      busy = false;
    }
  }

  async function confirm(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    const group = typed.trim();
    if (busy || group.length === 0) return;
    busy = true;
    message = "";
    failed = false;
    try {
      if (await onConfirm(group)) {
        typed = "";
      } else {
        failed = true;
        message = t("sync.kit.confirmMismatch");
      }
    } catch (cause: unknown) {
      console.warn("recovery kit could not be confirmed", cause);
      failed = true;
      message = t("sync.kit.confirmFailed");
    } finally {
      busy = false;
    }
  }
</script>

<div class="flex min-w-0 flex-col gap-2">
  <Notice tone="warning" role="status" copy={false} class="px-1 text-[0.8rem] leading-5" message={t("sync.kit.pending")} />
  <div class="px-1" aria-label={t("sync.kit.code")}>
    {#each codeLines as line, index (index)}
      <p class="font-mono text-[0.933333rem] font-semibold tracking-wider text-foreground select-all" dir="ltr">{line}</p>
    {/each}
  </div>
  {#if !kit.active}
    <p class="px-1 text-[0.8rem] leading-5 text-muted-foreground">{t("sync.kit.inactive")}</p>
  {/if}
  {#if exportable}
    <div class="flex flex-wrap gap-2">
      <button type="button" class={buttonClass} disabled={busy || printedAtMs !== null} onclick={() => void print()}>
        {t("sync.kit.print")}
      </button>
      <button type="button" class={buttonClass} disabled={busy} onclick={() => void saveFile()}>
        {t("sync.kit.saveFile")}
      </button>
    </div>
  {/if}
  <form class="flex min-w-0 flex-col gap-2 px-1" onsubmit={(event) => void confirm(event)}>
    <div class="text-[0.8rem] text-foreground">{t("sync.kit.confirmSavedTitle")}</div>
    <div class="text-[0.75rem] text-muted-foreground">{t("sync.kit.confirmSavedMessage")}</div>
    <label class="text-[0.8rem] text-muted-foreground" for="sync-recovery-kit-last-group">{t("sync.kit.confirmLabel")}</label>
    <input
      id="sync-recovery-kit-last-group"
      class="field h-9 w-full max-w-48 px-3 font-mono text-sm"
      dir="ltr"
      autocomplete="off"
      autocapitalize="characters"
      spellcheck="false"
      bind:value={typed}
      disabled={busy}
    />
    <div class="flex flex-wrap gap-2">
      <button type="submit" class={buttonClass} disabled={busy || typed.trim().length === 0}>
        {t("sync.kit.confirmSaved")}
      </button>
    </div>
  </form>
  {#if message}
    {#if failed}
      <Notice class="px-1 text-[0.8rem] leading-5" {message} copy={false} />
    {:else}
      <p role="status" class="px-1 text-[0.8rem] leading-5 text-muted-foreground">{message}</p>
    {/if}
  {/if}
</div>

{#if printedAtMs !== null}
  <section class="sync-kit-print" use:portal>
    <h1 class="sync-kit-print-title">{t("sync.kit.fileTitle")}</h1>
    <p>{t("sync.kit.fileCreated", formatDateTime(localization.locale, printedAtMs, { dateStyle: "long", timeStyle: "short" }))}</p>
    <p>{t("sync.kit.fileWarning")}</p>
    <svg
      class="sync-kit-print-qr"
      viewBox={`${-QUIET_ZONE_MODULES} ${-QUIET_ZONE_MODULES} ${qrSide} ${qrSide}`}
      role="img"
      aria-label={t("sync.kit.qrLabel")}
      shape-rendering="crispEdges"
    >
      <rect x={-QUIET_ZONE_MODULES} y={-QUIET_ZONE_MODULES} width={qrSide} height={qrSide} fill="white" />
      {#each kit.qr.modules as enabled, index (index)}
        {#if enabled}
          <rect x={index % kit.qr.width} y={Math.floor(index / kit.qr.width)} width="1" height="1" fill="black" />
        {/if}
      {/each}
    </svg>
    <p>{t("sync.kit.fileCode")}</p>
    {#each codeLines as line, index (index)}
      <p class="sync-kit-print-code" dir="ltr">{line}</p>
    {/each}
  </section>
{/if}

<style>
  /* The page exists only while it prints; the screen never shows it. */
  @media screen {
    :global(.sync-kit-print) {
      display: none;
    }
  }

  @media print {
    :global(html.sync-kit-printing body > :not(.sync-kit-print)) {
      display: none !important;
    }

    :global(html.sync-kit-printing .sync-kit-print) {
      display: block;
      padding: 1.5cm;
      color: black;
      background: white;
      font-size: 11pt;
      line-height: 1.5;
    }

    :global(html.sync-kit-printing .sync-kit-print-title) {
      margin-bottom: 0.5cm;
      font-size: 18pt;
      font-weight: 600;
    }

    :global(html.sync-kit-printing .sync-kit-print-qr) {
      width: 6cm;
      height: 6cm;
      margin: 0.8cm 0;
    }

    :global(html.sync-kit-printing .sync-kit-print-code) {
      font-family: ui-monospace, monospace;
      font-size: 14pt;
      font-weight: 600;
      letter-spacing: 0.08em;
    }
  }
</style>
