<script lang="ts">
  import Check from "@lucide/svelte/icons/check";
  import Copy from "@lucide/svelte/icons/copy";
  import { onMount } from "svelte";
  import type { PairingInvitation } from "$lib/api/vault-handoff";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { writeTextToClipboard } from "$lib/utils/clipboard";
  import { maskCode } from "$lib/utils/masked-code";
  import { formatPairingCountdown } from "$lib/vault/handoff-onboarding";

  /**
   * A pairing invitation as its QR code or as its text code to copy, with the time left before it
   * refreshes. Both views share the countdown, so the invitation refreshes whichever one shows.
   */
  let {
    invitation,
    view = "qr",
    onExpired,
  }: {
    invitation: PairingInvitation;
    /** Whether the QR code or the text code shows. */
    view?: "qr" | "code";
    onExpired?: () => void;
  } = $props();

  const COPY_FEEDBACK_MS = 2_000;
  const { t } = getLocalization();
  const quietZoneModules = 4;
  let now = $state(Date.now());
  let refreshRequestedFor = $state<string | null>(null);
  let copied = $state(false);
  let copyFailed = $state(false);
  let copyFeedbackTimer: number | undefined;
  const remaining = $derived(Math.max(0, invitation.expiresAtMs - now));

  $effect(() => {
    if (remaining > 0 || refreshRequestedFor === invitation.invitation) return;
    refreshRequestedFor = invitation.invitation;
    onExpired?.();
  });

  onMount(() => {
    const timer = window.setInterval(() => { now = Date.now(); }, 250);
    return () => {
      window.clearInterval(timer);
      window.clearTimeout(copyFeedbackTimer);
    };
  });

  async function copyCode(): Promise<void> {
    copyFailed = false;
    try {
      await writeTextToClipboard(invitation.invitation);
    } catch (cause: unknown) {
      console.warn("the pairing code could not be copied", cause);
      copyFailed = true;
      return;
    }
    copied = true;
    window.clearTimeout(copyFeedbackTimer);
    copyFeedbackTimer = window.setTimeout(() => { copied = false; }, COPY_FEEDBACK_MS);
  }
</script>

<div class="grid justify-items-center gap-3">
  {#if view === "code"}
    <!-- The code shows only by its ends, enough to compare with what the phone pasted. -->
    <div class="flex h-9 w-full items-center gap-2 rounded-md bg-muted ps-3 pe-1">
      <code dir="ltr" class="min-w-0 flex-1 truncate font-mono text-[0.8rem] text-foreground">{maskCode(invitation.invitation)}</code>
      <button
        type="button"
        class="inline-flex h-7 shrink-0 items-center gap-1.5 rounded-sm px-2 text-[0.8rem] font-medium text-foreground hover:bg-background"
        onclick={() => void copyCode()}
      >
        {#if copied}<Check size={14} aria-hidden="true" />{:else}<Copy size={14} aria-hidden="true" />{/if}
        {copied ? t("vaultHandoff.codeCopied") : t("vaultHandoff.copyCode")}
      </button>
    </div>
    {#if copyFailed}
      <p role="alert" class="text-center text-[0.8rem] leading-5 text-destructive">{t("vaultHandoff.copyCodeFailed")}</p>
    {/if}
  {:else}
    <svg
      viewBox={`${-quietZoneModules} ${-quietZoneModules} ${invitation.qr.width + quietZoneModules * 2} ${invitation.qr.width + quietZoneModules * 2}`}
      class="aspect-square w-full max-w-80 bg-white"
      role="img"
      aria-label={t("vaultHandoff.qrAlt")}
      shape-rendering="crispEdges"
    >
      <rect
        x={-quietZoneModules}
        y={-quietZoneModules}
        width={invitation.qr.width + quietZoneModules * 2}
        height={invitation.qr.width + quietZoneModules * 2}
        fill="white"
      />
      {#each invitation.qr.modules as enabled, index}
        {#if enabled}
          <rect
            x={index % invitation.qr.width}
            y={Math.floor(index / invitation.qr.width)}
            width="1"
            height="1"
            fill="black"
          />
        {/if}
      {/each}
    </svg>
  {/if}
  <p class="h-5 text-center text-[0.8rem] tabular-nums text-muted-foreground" aria-live="polite">
    {remaining > 0
      ? t("vaultHandoff.refreshesIn", formatPairingCountdown(remaining))
      : t("vaultHandoff.refreshingCode")}
  </p>
</div>
