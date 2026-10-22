<script lang="ts">
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import { getLocalization } from "$lib/i18n/translator.svelte";

  /** Shows the code an enrolling device waits on while the coordinator user compares it. */
  let { code }: { code: string } = $props();

  const { t } = getLocalization();
</script>

<div class="flex w-full flex-col items-center gap-3 text-center" role="status">
  <p class="text-base font-semibold text-foreground">{t("vaultHandoff.enrollmentWaitingTitle")}</p>
  <div class="space-y-1">
    <p class="text-[0.8rem] text-muted-foreground">{t("vaultHandoff.enrollmentCode")}</p>
    <p class="font-mono text-2xl font-semibold tracking-wider text-foreground" dir="ltr">{code}</p>
  </div>
  <p class="mx-auto max-w-sm text-[0.8rem] leading-5 text-muted-foreground">
    {t("vaultHandoff.enrollmentWaiting")}
  </p>
  <LoaderCircle size={18} strokeWidth={1.8} class="animate-spin text-muted-foreground" aria-hidden="true" />
</div>
