<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import CircleCheck from "@lucide/svelte/icons/circle-check";
  import Loading from "$lib/components/ui/Loading.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { appSessionStartedAt } from "$lib/stores/app-session";
  import { getDistractionsExtensionConnection } from "$lib/stores/distractions-extension-status.svelte";

  const EXTENSION_STARTUP_GRACE_MS = 45_000;
  const EXTENSION_STARTUP_PENDING_REASONS = new Set([
    "connection is from an older app session",
    "no extension connection has been recorded",
  ]);
  const NOTICE_CLASS = "px-1 text-[0.8rem] leading-5";

  const extensionConnection = getDistractionsExtensionConnection();
  const { t } = getLocalization();

  const extensionStatus = $derived(extensionConnection.status);
  const extensionStatusLoading = $derived(extensionConnection.loading);
  const extensionStatusError = $derived(extensionConnection.error);
  const extensionStatusWaitingForFirstConnection = $derived.by(() => {
    if (!extensionStatus || extensionStatus.connected) return false;
    if (!extensionStatus.reason || !EXTENSION_STARTUP_PENDING_REASONS.has(extensionStatus.reason)) {
      return false;
    }
    const sessionStartedAtMs = Date.parse(appSessionStartedAt);
    const checkedAtMs = Date.parse(extensionStatus.checkedAt);
    if (!Number.isFinite(sessionStartedAtMs) || !Number.isFinite(checkedAtMs)) return false;
    return checkedAtMs - sessionStartedAtMs <= EXTENSION_STARTUP_GRACE_MS;
  });

  /** Retries a failed status read and reports whether the extension status is readable again. */
  async function retryExtensionStatus(): Promise<boolean> {
    await extensionConnection.refresh();
    return extensionConnection.error === null;
  }

  async function openExtensionInstallDocs(): Promise<void> {
    try {
      await invoke("distractions_open_extension_install_docs");
    } catch (err) {
      console.warn("Failed to open extension install docs:", err);
    }
  }

  onMount(() => {
    const stop = extensionConnection.start();
    const resume = () => extensionConnection.resume();
    const resumeWhenVisible = () => {
      if (document.visibilityState === "visible") resume();
    };
    window.addEventListener("focus", resume);
    document.addEventListener("visibilitychange", resumeWhenVisible);
    return () => {
      window.removeEventListener("focus", resume);
      document.removeEventListener("visibilitychange", resumeWhenVisible);
      stop();
    };
  });
</script>

{#if !extensionStatus && extensionStatusError}
  <Notice
    class={NOTICE_CLASS}
    message={t("settings.distractions.extension.unavailable")}
    copy={`${t("settings.distractions.extension.unavailable")}: ${extensionStatusError}`}
    onRetry={retryExtensionStatus}
  />
{:else if !extensionStatus && extensionStatusLoading}
  <div class={NOTICE_CLASS}><Loading layout="inline" showLabel label={t("settings.distractions.extension.checking")} /></div>
{:else if extensionStatusWaitingForFirstConnection}
  <div class={NOTICE_CLASS}><Loading layout="inline" showLabel label={t("settings.distractions.extension.waiting")} /></div>
{:else if extensionStatus?.connected}
  <Notice
    class={NOTICE_CLASS}
    icon={CircleCheck}
    role="status"
    copy={false}
    message={t("settings.distractions.extension.connected")}
  />
{:else if extensionStatus}
  <Notice
    class={NOTICE_CLASS}
    tone="warning"
    role="status"
    copy={false}
    message={t("settings.distractions.extension.notConnected")}
  >
    {#snippet actions()}
      <button type="button" class="notice-action-text" onclick={() => void openExtensionInstallDocs()}>
        {t("settings.distractions.extension.install")}
      </button>
    {/snippet}
  </Notice>
{/if}
