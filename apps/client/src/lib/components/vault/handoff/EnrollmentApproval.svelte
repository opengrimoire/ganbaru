<script lang="ts">
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import Monitor from "@lucide/svelte/icons/monitor";
  import Smartphone from "@lucide/svelte/icons/smartphone";
  import type { PendingEnrollment } from "$lib/api/vault-handoff";
  import { getLocalization } from "$lib/i18n/translator.svelte";

  /** Asks the coordinator user to compare the code an enrolling device shows before linking it. */
  let {
    pending,
    busy,
    onRespond,
  }: {
    pending: PendingEnrollment;
    busy: boolean;
    onRespond: (approve: boolean) => void;
  } = $props();

  const { t } = getLocalization();
</script>

<div class="flex w-full flex-col items-center gap-4 text-center" role="group" aria-labelledby="enrollment-approval-title">
  <div class="space-y-1">
    <p id="enrollment-approval-title" class="text-base font-semibold text-foreground">
      {t("vaultHandoff.enrollmentApprovalTitle")}
    </p>
    <p class="flex min-w-0 items-center justify-center gap-1.5 text-sm text-foreground">
      {#if pending.deviceKind === "computer"}
        <Monitor size={15} strokeWidth={1.8} class="shrink-0 opacity-70" aria-hidden="true" />
      {:else}
        <Smartphone size={15} strokeWidth={1.8} class="shrink-0 opacity-70" aria-hidden="true" />
      {/if}
      <span class="min-w-0 truncate" dir="auto">{pending.deviceLabel}</span>
    </p>
  </div>
  <div class="space-y-1">
    <p class="text-[0.8rem] text-muted-foreground">{t("vaultHandoff.enrollmentCode")}</p>
    <p class="font-mono text-2xl font-semibold tracking-wider text-foreground" dir="ltr">{pending.code}</p>
  </div>
  <p class="mx-auto max-w-sm text-[0.8rem] leading-5 text-muted-foreground">
    {t("vaultHandoff.enrollmentApprovalInstructions")}
  </p>
  <div class="flex flex-wrap justify-center gap-2">
    <button
      type="button"
      class="inline-flex min-h-9 items-center justify-center rounded-md border border-border px-3 text-[0.8rem] font-medium hover:bg-accent disabled:pointer-events-none disabled:opacity-55"
      disabled={busy}
      onclick={() => onRespond(false)}
    >
      {t("vaultHandoff.enrollmentDecline")}
    </button>
    <button
      type="button"
      class="inline-flex min-h-9 items-center justify-center gap-1.5 rounded-md bg-primary px-3 text-[0.8rem] font-medium text-primary-foreground hover:bg-primary/90 disabled:pointer-events-none disabled:opacity-55"
      disabled={busy}
      onclick={() => onRespond(true)}
    >
      {#if busy}<LoaderCircle size={14} class="animate-spin" />{/if}
      {t("vaultHandoff.enrollmentApprove")}
    </button>
  </div>
</div>
