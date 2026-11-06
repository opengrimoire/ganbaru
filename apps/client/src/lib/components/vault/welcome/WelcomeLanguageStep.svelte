<script lang="ts">
  import type { Snippet } from "svelte";
  import type { AppLocale, LanguagePreference } from "$lib/i18n/locales";
  import {
    browserLocaleCandidates,
    plannedLanguageEntries,
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
  const planned = $derived(plannedLanguageEntries(localization.locale));

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

<section class="@container flex h-full flex-col">
  <div class="min-h-0 flex-1 overflow-x-hidden overflow-y-auto">
    <div class="mx-auto flex w-full max-w-xl flex-col gap-8 px-4 pb-6 pt-12 @min-[560px]:px-8 @min-[560px]:pt-18 @min-[860px]:gap-10 @min-[860px]:pt-26">
      <div class="space-y-2 @min-[860px]:space-y-3">
        <h1 id="{headingId}-heading" class="text-2xl font-semibold leading-tight tracking-tight text-balance text-foreground @min-[560px]:text-3xl @min-[860px]:text-4xl">
          {t("vaultSetup.language.headline")}
        </h1>
        <p class="text-sm leading-6 text-pretty text-muted-foreground @min-[560px]:text-base @min-[560px]:leading-7">
          {t("vaultSetup.language.body")}
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

      <div class="space-y-2">
        <h2 id="{headingId}-planned" class="px-4 text-sm font-medium text-muted-foreground">
          {t("vaultSetup.language.plannedHeading")}
        </h2>
        <ul aria-labelledby="{headingId}-planned" class="grid gap-1.5 @min-[480px]:grid-cols-2">
          {#each planned as entry (entry.id)}
            <li class="min-w-0 rounded-lg px-4 py-2 opacity-55">
              <span lang={entry.id} dir={entry.direction} class="block truncate text-start text-sm font-medium text-foreground">
                {entry.title}
              </span>
              <span class="block truncate text-xs text-muted-foreground">{entry.detail}</span>
            </li>
          {/each}
        </ul>
      </div>
    </div>
  </div>

  <div class="mx-auto w-full max-w-xl shrink-0 px-4 pb-6 @min-[560px]:px-8 @min-[860px]:pb-10">
    <div class="border-t border-border pt-4">{@render actions()}</div>
  </div>
</section>
