<!--
  @component
  The composer's status and MCP panels. The caller reads the session it talks to into a
  `ComposerStatusRead`, so the panel reads no store.
-->
<script lang="ts">
  import type { McpStatusRead, ProviderSessionState } from "$lib/chat/contracts";
  import type { ComposerStatusRead } from "$lib/chat/composer/composer-meter";
  import { formatDateTime, formatNumber } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";
  import Loading from "$lib/components/ui/Loading.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";

  let {
    panel,
    status,
    mcpStatus,
    loading,
    error,
    onClose,
  }: {
    panel: "status" | "mcp";
    status: ComposerStatusRead;
    mcpStatus: McpStatusRead | null;
    loading: boolean;
    error: string | null;
    onClose: () => void;
  } = $props();

  const localization = getLocalization();
  const { t } = localization;
  const meter = $derived(status.usage.meter);
  const rateLimitWindows = $derived(status.usage.windows);
  const title = $derived(panel === "mcp"
    ? t("chat.composer.mcpPanelTitle")
    : t("chat.composer.statusPanelTitle"));

  function accountStatusLabel(): string {
    if (status.accountLabel && status.planLabel) return `${status.accountLabel} · ${status.planLabel}`;
    return status.accountLabel ?? status.planLabel ?? t("chat.composer.statusAccountUnavailable");
  }

  function contextRemainingLabel(): string {
    if (!meter || meter.maximumTokens === null || meter.ratio === null) {
      return contextLabel() ?? t("chat.composer.contextUnavailable");
    }
    return t(
      "chat.composer.contextRemaining",
      contextPercentage(1 - meter.ratio),
      formatNumber(localization.locale, meter.usedTokens),
      formatNumber(localization.locale, meter.maximumTokens),
    );
  }

  function contextLabel(): string | null {
    if (!meter) return null;
    const used = formatNumber(localization.locale, meter.usedTokens);
    return meter.maximumTokens === null
      ? t("chat.composer.contextUnknown", used)
      : t("chat.composer.contextUsed", used, formatNumber(localization.locale, meter.maximumTokens, { notation: "compact" }));
  }

  function contextPercentage(value: number): string {
    return formatNumber(localization.locale, value * 100, { maximumFractionDigits: 0 });
  }

  function resetTimeLabel(seconds: number | null): string | null {
    if (seconds === null) return null;
    return formatDateTime(localization.locale, seconds * 1_000, {
      month: "short",
      day: "numeric",
      hour: "numeric",
      minute: "2-digit",
    });
  }

  function readableProviderStatus(value: string | null): string {
    if (!value) return t("chat.composer.mcpConfigured");
    const words = value.replaceAll(/[_-]+/g, " ").trim();
    return words
      ? `${words.charAt(0).toUpperCase()}${words.slice(1)}`
      : t("chat.composer.mcpConfigured");
  }

  function providerSessionLabel(state: ProviderSessionState): string {
    switch (state) {
      case "stopped": return t("chat.composer.statusSessionStopped");
      case "starting": return t("chat.composer.statusSessionStarting");
      case "ready": return t("chat.composer.statusSessionReady");
      case "active": return t("chat.composer.statusSessionActive");
      case "waiting_for_approval": return t("chat.composer.statusSessionApproval");
      case "waiting_for_user_input": return t("chat.composer.statusSessionInput");
      case "stopping": return t("chat.composer.statusSessionStopping");
      case "failed": return t("chat.composer.statusSessionFailed");
    }
  }
</script>

<div class="composer-info-panel surface-floating" role="region" aria-label={title}>
  <header>
    <strong>{title}</strong>
    <button type="button" onclick={onClose}>{t("chat.composer.closePanel")}</button>
  </header>
  <div class="composer-info-body" use:scrollEdgeFadeAction>
    {#if panel === "status"}
      <dl class="status-panel-grid">
        <div><dt>{t("chat.composer.statusProvider")}</dt><dd>{status.providerLabel ?? t("chat.composer.statusUnknown")}</dd></div>
        <div><dt>{t("chat.composer.statusSession")}</dt><dd><code title={status.sessionId ?? undefined}>{status.sessionId ?? t("chat.composer.statusNoSession")}</code><span class="status-state">{providerSessionLabel(status.sessionState)}</span></dd></div>
        <div><dt>{t("chat.composer.statusModel")}</dt><dd>{status.modelLabel ?? t("chat.composer.statusProviderManaged")}</dd></div>
        <div><dt>{t("chat.composer.statusAccount")}</dt><dd>{accountStatusLabel()}</dd></div>
        <div class="status-meter-row">
          <dt>{t("chat.composer.statusContext")}</dt>
          <dd>
            <span>{contextRemainingLabel()}</span>
            {#if meter && meter.ratio !== null}<span class="panel-meter" style={`--panel-progress:${Math.max(0, 1 - meter.ratio)}`}><span></span></span>{/if}
          </dd>
        </div>
        {#each rateLimitWindows as window (window.id)}
          <div class="status-meter-row">
            <dt>{window.label}</dt>
            <dd>
              <span>{window.usedPercent === null ? t("chat.composer.rateLimitAvailable") : t("chat.composer.rateLimitRemaining", formatNumber(localization.locale, 100 - window.usedPercent, { maximumFractionDigits: 0 }))}{#if resetTimeLabel(window.resetsAtSeconds)} <small>{t("chat.composer.rateLimitResets", resetTimeLabel(window.resetsAtSeconds) ?? "")}</small>{/if}</span>
              {#if window.usedPercent !== null}<span class="panel-meter" style={`--panel-progress:${Math.max(0, 1 - window.usedPercent / 100)}`}><span></span></span>{/if}
            </dd>
          </div>
        {/each}
      </dl>
    {:else if loading}
      <Loading layout="fill" class="min-h-18" />
    {:else if error}
      <Notice class="panel-error" message={error} />
    {:else if !mcpStatus || mcpStatus.servers.length === 0}
      <p class="panel-state">{t("chat.composer.mcpEmpty")}</p>
    {:else}
      <div class="mcp-status-table" aria-label={t("chat.composer.mcpPanelTitle")}>
        {#each mcpStatus.servers as server (server.name)}
          <div>
            <strong title={server.name}>{server.name}</strong>
            <span>{t("chat.composer.mcpAuthStatus", readableProviderStatus(server.authStatus))}</span>
            <span class:enabled={server.enabled}>{server.enabled ? t("chat.composer.enabled") : t("chat.composer.disabled")}</span>
          </div>
        {/each}
      </div>
    {/if}
  </div>
</div>

<style>
  .composer-info-panel { position: absolute; inset-inline: 0; bottom: calc(100% + 0.4rem); z-index: 30; display: flex; max-height: min(22rem, calc(100dvh - 7rem)); flex-direction: column; overflow: hidden; margin-inline: 0 !important; }
  .composer-info-body { min-height: 0; flex: 1 1 auto; overflow: auto; overscroll-behavior: contain; }
  .composer-info-panel > header { display: flex; min-height: 2.6rem; align-items: center; justify-content: space-between; gap: 1rem; border-bottom: 1px solid color-mix(in srgb, var(--border) 72%, transparent); flex: 0 0 auto; padding: 0.55rem 0.9rem; }
  .composer-info-panel > header strong { font-weight: 600; }
  .composer-info-panel > header button { border-radius: var(--floating-item-radius); padding: 0.2rem 0.35rem; color: var(--muted-foreground); font-size: var(--panel-detail-font-size); }
  .composer-info-panel > header button:hover { background: var(--accent); color: var(--foreground); }
  .status-panel-grid { display: grid; padding: 0.55rem 0.9rem 0.75rem; }
  .status-panel-grid > div { display: grid; min-width: 0; grid-template-columns: 6.5rem minmax(0, 1fr); align-items: baseline; gap: 0.75rem; padding-block: 0.28rem; }
  .status-panel-grid dt { color: var(--muted-foreground); }
  .status-panel-grid dd { display: flex; min-width: 0; align-items: center; gap: 0.55rem; margin: 0; overflow: hidden; }
  .status-panel-grid code { overflow: hidden; color: inherit; font: inherit; text-overflow: ellipsis; white-space: nowrap; }
  .status-state { flex: 0 0 auto; border-radius: 999px; background: var(--muted); padding: 0.08rem 0.38rem; color: var(--muted-foreground); font-size: var(--panel-detail-font-size); }
  .status-panel-grid > .status-meter-row { align-items: start; }
  .status-meter-row dd { display: grid; gap: 0.28rem; }
  .status-meter-row dd > span:first-child { display: flex; min-width: 0; justify-content: space-between; gap: 0.75rem; }
  .status-meter-row small { color: var(--muted-foreground); font-size: var(--panel-detail-font-size); }
  .panel-meter { display: block; width: 100%; height: 0.38rem; overflow: hidden; border-radius: 999px; background: color-mix(in srgb, var(--muted-foreground) 18%, transparent); }
  .panel-meter > span { display: block; width: calc(var(--panel-progress) * 100%); height: 100%; border-radius: inherit; background: color-mix(in srgb, var(--foreground) 72%, var(--muted-foreground)); }
  .mcp-status-table { display: grid; padding-block: 0.4rem; }
  .mcp-status-table > div { display: grid; min-width: 0; grid-template-columns: minmax(8rem, 1fr) minmax(8rem, 1fr) auto; align-items: center; gap: 1rem; padding: 0.36rem 0.9rem; }
  .mcp-status-table > div:hover { background: color-mix(in srgb, var(--accent) 52%, transparent); }
  .mcp-status-table strong { overflow: hidden; font-family: var(--font-mono, monospace); font-weight: 400; text-overflow: ellipsis; white-space: nowrap; }
  .mcp-status-table span { color: var(--muted-foreground); }
  .mcp-status-table span:last-child { justify-self: end; color: var(--destructive); }
  .mcp-status-table span.enabled { color: var(--foreground); }
  .panel-state { display: flex; min-height: 4.5rem; align-items: center; gap: 0.5rem; padding: 0.75rem 0.9rem; color: var(--muted-foreground); }
  .composer-info-panel :global(.panel-error) { min-height: 4.5rem; align-items: center; padding: 0.75rem 0.9rem; }
  @container chat-composer (max-width: 390px) { .status-panel-grid > div { grid-template-columns: 5rem minmax(0, 1fr); } .mcp-status-table > div { grid-template-columns: minmax(0, 1fr) auto; gap: 0.65rem; } .mcp-status-table span:nth-child(2) { grid-column: 1 / -1; grid-row: 2; } }
</style>
