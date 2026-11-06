<script lang="ts">
  import type { Snippet } from "svelte";
  import SettingsPath from "$lib/components/settings/SettingsPath.svelte";
  import type { AppLocale, LanguagePreference } from "$lib/i18n/locales";
  import {
    browserLocaleCandidates,
    preVaultLanguageChoices,
    writePreVaultLanguagePreference,
  } from "$lib/i18n/pre-vault-language";
  import { getLocalization, type TranslationKey } from "$lib/i18n/translator.svelte";
  import { cn } from "$lib/utils";

  /**
   * The first welcome screen: the app's language. Each language is named in its own words, and choosing one applies
   * it at once, so the rest of the welcome reads in it. The choice is remembered until vault setup saves it.
   */
  let { actions }: { actions: Snippet } = $props();

  const localization = getLocalization();
  const { t } = localization;
  const headingId = $props.id();

  const LANGUAGE_NAME_KEYS = {
    en: "language.englishOption",
    es: "language.spanishOption",
  } as const satisfies Record<AppLocale, TranslationKey>;

  const systemCandidates = browserLocaleCandidates(typeof navigator === "undefined" ? undefined : navigator);
  const preference = $derived(localization.languagePreference);
  const choices = $derived(
    preVaultLanguageChoices(systemCandidates, t("language.systemOption"), (locale) => t(LANGUAGE_NAME_KEYS[locale])),
  );

  function safeStorage(): Storage | undefined {
    try {
      return window.localStorage;
    } catch {
      return undefined;
    }
  }

  async function choose(value: LanguagePreference): Promise<void> {
    const applied = await localization.setLanguagePreference(value, { persist: false });
    if (applied) writePreVaultLanguagePreference(safeStorage(), value);
  }
</script>

<section class="@container h-full overflow-x-hidden overflow-y-auto">
  <div class="mx-auto flex min-h-full w-full max-w-xl flex-col px-4 pb-6 pt-8 @min-[560px]:px-8 @min-[860px]:py-10">
    <div class="flex flex-1 flex-col gap-8 pb-6 pt-4 @min-[560px]:pt-10 @min-[860px]:gap-10 @min-[860px]:pt-16">
      <div class="space-y-2 @min-[860px]:space-y-3">
        <h1 id="{headingId}-heading" class="text-2xl font-semibold leading-tight tracking-tight text-balance text-foreground @min-[560px]:text-3xl @min-[860px]:text-4xl">
          {t("vaultSetup.language.headline")}
        </h1>
        <p class="text-sm leading-6 text-pretty text-muted-foreground @min-[560px]:text-base @min-[560px]:leading-7">
          {t("vaultSetup.language.body")}
          <SettingsPath trail={[t("settings.section.appearance")]} />
        </p>
      </div>

      <div role="radiogroup" aria-labelledby="{headingId}-heading" class="grid gap-1.5">
        {#each choices as choice (choice.value)}
          {@const selected = choice.value === preference}
          <label
            class={cn(
              "flex min-h-14 cursor-pointer items-center gap-4 rounded-lg px-4 py-2.5 transition-colors has-focus-visible:bg-accent",
              selected ? "bg-accent" : "hover:bg-accent/60",
            )}
          >
            <input
              type="radio"
              name="{headingId}-language"
              value={choice.value}
              checked={selected}
              onchange={() => void choose(choice.value)}
              class="sr-only"
            />
            <span class="min-w-0 flex-1">
              <span lang={choice.titleLang ?? undefined} class="block truncate text-base font-medium text-foreground">
                {choice.title}
              </span>
              <span lang={choice.detailLang ?? undefined} class="block truncate text-sm text-muted-foreground">
                {choice.detail}
              </span>
            </span>
            <span
              aria-hidden="true"
              class={cn(
                "flex size-5 shrink-0 items-center justify-center rounded-full border-2 transition-colors",
                selected ? "border-primary" : "border-foreground/25",
              )}
            >
              {#if selected}<span class="size-2.5 rounded-full bg-primary"></span>{/if}
            </span>
          </label>
        {/each}
      </div>
    </div>

    <div class="shrink-0 border-t border-border pt-4">{@render actions()}</div>
  </div>
</section>
