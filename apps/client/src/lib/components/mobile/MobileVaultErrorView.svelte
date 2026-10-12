<script lang="ts">
  import HardDrive from "@lucide/svelte/icons/hard-drive";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";

  let { message, onRetry }: { message: string; onRetry: () => unknown } = $props();
  const { t } = getLocalization();
</script>

<main
  class="mobile-viewport-height flex w-screen bg-background pb-(--safe-area-bottom) pt-(--safe-area-top) text-foreground"
  style="padding-left: calc(var(--safe-area-left) + 1.5rem); padding-right: calc(var(--safe-area-right) + 1.5rem);"
>
  <Notice layout="fill" class="text-base" icon={HardDrive} title={t("mobile.startupFailed")} {message} {onRetry} />
</main>
