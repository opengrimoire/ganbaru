<script lang="ts">
  import HardDrive from "@lucide/svelte/icons/hard-drive";
  import DatabaseRollbacks from "$lib/components/vault/DatabaseRollbacks.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";

  /**
   * The screen shown when the active vault cannot open at startup.
   *
   * @param message - Localized guidance for the failure.
   * @param onRetry - Opens the app again.
   * @param offerRollbacks - Lists the vault's rollback copies to restore, for a vault whose database is gone.
   */
  let {
    message,
    onRetry,
    offerRollbacks = false,
  }: { message: string; onRetry: () => unknown; offerRollbacks?: boolean } = $props();
  const { t } = getLocalization();
</script>

<main
  class="mobile-viewport-height flex w-screen bg-background pb-(--safe-area-bottom) pt-(--safe-area-top) text-foreground"
  style="padding-left: calc(var(--safe-area-left) + 1.5rem); padding-right: calc(var(--safe-area-right) + 1.5rem);"
>
  {#if offerRollbacks}
    <div class="mx-auto flex min-h-0 w-full max-w-md flex-col gap-6 overflow-y-auto py-8">
      <Notice class="text-base" icon={HardDrive} title={t("mobile.startupFailed")} {message} {onRetry} />
      <DatabaseRollbacks onRestored={onRetry} />
    </div>
  {:else}
    <Notice layout="fill" class="text-base" icon={HardDrive} title={t("mobile.startupFailed")} {message} {onRetry} />
  {/if}
</main>
