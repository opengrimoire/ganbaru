<script lang="ts">
  import type { Snippet } from "svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import WelcomeFilm from "./WelcomeFilm.svelte";

  /**
   * The Calendar's welcome screen: the film of a Focus session planned in the Calendar and run from it, next to the
   * pitch. Wide containers place the film beside the text; narrow ones stack it above, and it steps aside when there
   * is no room for it.
   */
  let { header, actions }: { header: Snippet; actions: Snippet } = $props();

  const { t } = getLocalization();
</script>

<section class="@container h-full overflow-x-hidden overflow-y-auto">
  <div
    class="mx-auto flex min-h-full w-full max-w-300 flex-col px-4 pb-6 pt-8 @min-[560px]:px-8 @min-[860px]:grid @min-[860px]:grid-cols-[minmax(18rem,26rem)_minmax(0,1fr)] @min-[860px]:grid-rows-[auto_minmax(0,1fr)] @min-[860px]:gap-x-10 @min-[860px]:px-12 @min-[860px]:py-10"
  >
    <div class="flex shrink-0 items-start">{@render header()}</div>

    <WelcomeFilm class="min-h-0 flex-1 py-3 @min-[860px]:col-start-2 @min-[860px]:row-span-2 @min-[860px]:row-start-1 @min-[860px]:py-0" />

    <div class="shrink-0 space-y-5 @min-[860px]:col-start-1 @min-[860px]:row-start-2 @min-[860px]:self-center @min-[860px]:space-y-8">
      <div class="space-y-2 @min-[860px]:space-y-4">
        <h1 class="text-2xl font-semibold leading-tight tracking-tight text-balance text-foreground @min-[560px]:text-3xl @min-[860px]:text-4xl">
          {t("vaultSetup.welcomeHeadline")}
        </h1>
        <p class="text-sm leading-6 text-pretty text-muted-foreground @min-[560px]:text-base @min-[560px]:leading-7">
          {t("vaultSetup.welcomeBody")}
        </p>
      </div>
      {@render actions()}
    </div>
  </div>
</section>
