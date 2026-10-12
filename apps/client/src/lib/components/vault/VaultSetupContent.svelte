<script lang="ts">
  import Folder from "@lucide/svelte/icons/folder";
  import type { Snippet } from "svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import VaultLanguageDropdown from "./VaultLanguageDropdown.svelte";

  const { t } = getLocalization();

  let {
    title = t("vaultSetup.title"),
    intro,
    developmentWarning = null,
    location,
    locationLabel = t("vaultSetup.defaultLocation"),
    actions,
    notice,
    error = null,
  }: {
    title?: string;
    intro: string;
    developmentWarning?: string | null;
    location: string;
    locationLabel?: string;
    actions: Snippet;
    notice?: Snippet;
    error?: string | null;
  } = $props();

</script>

<section class="h-full overflow-y-auto px-4 min-[560px]:px-8 min-[760px]:px-10">
  <div class="mx-auto grid min-h-full w-full max-w-2xl grid-rows-[minmax(4.5rem,1fr)_auto_minmax(4.5rem,1fr)] py-5 min-[760px]:py-8">
    <div class="flex items-end pb-4">
      <VaultLanguageDropdown />
    </div>

    <div class="flex flex-col gap-7">
      <div class="space-y-2">
        <h1 class="max-w-xl text-2xl font-semibold leading-tight text-foreground min-[560px]:text-3xl">
          {title}
        </h1>
        <p class="text-sm leading-6 text-muted-foreground">
          {intro}
          {#if developmentWarning}
            <br />
            <strong class="font-semibold text-warning">{developmentWarning}</strong>
          {/if}
        </p>
      </div>

      <div class="grid gap-5">
        <div class="grid gap-2 border-y border-border py-4 min-[560px]:grid-cols-[auto_1fr] min-[560px]:items-start">
          <div class="flex items-center gap-2 text-sm font-medium text-foreground">
            <Folder size={16} strokeWidth={1.8} aria-hidden="true" />
            {locationLabel}
          </div>
          <p class="break-all text-sm leading-5 text-muted-foreground min-[560px]:text-right">
            {location}
          </p>
        </div>

        {#if notice}
          {@render notice()}
        {/if}

        {@render actions()}

        {#if error}
          <Notice message={error} class="text-sm" />
        {/if}
      </div>
    </div>

    <div></div>
  </div>
</section>
