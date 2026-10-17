<script lang="ts">
  import { onMount } from "svelte";
  import type { PairingInvitation } from "$lib/api/vault-handoff";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { formatPairingCountdown } from "$lib/vault/handoff-onboarding";

  let {
    invitation,
    onExpired,
  }: {
    invitation: PairingInvitation;
    onExpired?: () => void;
  } = $props();

  const { t } = getLocalization();
  const quietZoneModules = 4;
  let now = $state(Date.now());
  let refreshRequestedFor = $state<string | null>(null);
  const remaining = $derived(Math.max(0, invitation.expiresAtMs - now));

  $effect(() => {
    if (remaining > 0 || refreshRequestedFor === invitation.invitation) return;
    refreshRequestedFor = invitation.invitation;
    onExpired?.();
  });

  onMount(() => {
    const timer = window.setInterval(() => { now = Date.now(); }, 250);
    return () => window.clearInterval(timer);
  });
</script>

<div class="grid justify-items-center gap-3">
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
  <p class="h-5 text-center text-[0.8rem] tabular-nums text-muted-foreground" aria-live="polite">
    {remaining > 0
      ? t("vaultHandoff.refreshesIn", formatPairingCountdown(remaining))
      : t("vaultHandoff.refreshingCode")}
  </p>
</div>
