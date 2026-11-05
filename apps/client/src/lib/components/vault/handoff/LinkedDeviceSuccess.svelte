<script lang="ts">
  import CircleCheck from "@lucide/svelte/icons/circle-check";
  import { fade, scale } from "svelte/transition";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { prefersReducedMotion } from "$lib/utils/smooth-swap";

  /** Confirms in onboarding that `device` is now linked, in place of the linking instructions. */
  let { device }: { device: string } = $props();

  const { t } = getLocalization();
  const duration = prefersReducedMotion() ? 0 : 220;
</script>

<div class="flex flex-col items-center gap-3 text-center" role="status" in:fade|global={{ duration }}>
  <div in:scale|global={{ duration, start: 0.6 }}>
    <CircleCheck size={48} strokeWidth={1.6} class="text-primary" aria-hidden="true" />
  </div>
  <h1 class="text-2xl font-semibold leading-tight text-foreground min-[560px]:text-3xl" dir="auto">
    {t("vaultHandoff.linkedTitle", device)}
  </h1>
  <p class="text-sm leading-6 text-muted-foreground">{t("vaultHandoff.linkedDescription")}</p>
</div>
