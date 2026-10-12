<script lang="ts">
  import { tick } from "svelte";
  import Check from "@lucide/svelte/icons/check";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import Languages from "@lucide/svelte/icons/languages";
  import Search from "@lucide/svelte/icons/search";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import {
    browserLocaleCandidates,
    languageMatchesQuery,
    languageOptionSearchText,
    readPreVaultLanguagePreference,
    resolvedSystemLanguageName,
    selectedLanguageName,
    supportedExplicitLocaleOptions,
    writePreVaultLanguagePreference,
    type LanguageOption,
  } from "$lib/i18n/pre-vault-language";
  import { cn } from "$lib/utils";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";

  const localization = getLocalization();
  const { t } = localization;

  let open = $state(false);
  let query = $state("");
  let rootEl = $state<HTMLDivElement | undefined>(undefined);
  let searchInputEl = $state<HTMLInputElement | undefined>(undefined);

  function safeStorage(): Storage | undefined {
    if (typeof window === "undefined") return undefined;
    try {
      return window.localStorage;
    } catch {
      return undefined;
    }
  }

  function safeNavigator(): Navigator | undefined {
    return typeof navigator === "undefined" ? undefined : navigator;
  }

  const initialPreference = readPreVaultLanguagePreference(safeStorage());
  if (initialPreference) {
    void localization.setLanguagePreference(initialPreference, { persist: false });
  }

  const locale = $derived(localization.locale);
  const preference = $derived(localization.languagePreference);
  const systemLanguageName = $derived(
    resolvedSystemLanguageName(browserLocaleCandidates(safeNavigator())),
  );
  const triggerLabel = $derived(selectedLanguageName(preference, locale));
  const options = $derived<LanguageOption[]>([
    {
      value: "system",
      label: `${t("language.systemOption")} (${systemLanguageName})`,
      searchText: languageOptionSearchText(
        "system",
        t("language.systemOption"),
        systemLanguageName,
      ),
    },
    ...supportedExplicitLocaleOptions().map((appLocale) => {
      const metadata = appLocale === "en"
        ? { label: t("language.englishOption"), nativeLabel: "English" }
        : { label: t("language.spanishOption"), nativeLabel: "Español" };
      return {
        value: appLocale,
        label: metadata.nativeLabel,
        searchText: languageOptionSearchText(
          appLocale,
          metadata.nativeLabel,
          metadata.label,
        ),
      };
    }),
  ]);
  const filteredOptions = $derived(
    options.filter((option) => languageMatchesQuery(option, query)),
  );

  async function toggle(): Promise<void> {
    open = !open;
    if (!open) return;
    query = "";
    await tick();
    searchInputEl?.focus();
  }

  async function selectLanguage(next: LanguageOption): Promise<void> {
    const applied = await localization.setLanguagePreference(next.value, { persist: false });
    if (!applied) return;
    writePreVaultLanguagePreference(safeStorage(), next.value);
    open = false;
    query = "";
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (!open || event.key !== "Escape") return;
    event.preventDefault();
    event.stopPropagation();
    open = false;
  }

  $effect(() => {
    if (!open) return;
    function handlePointerDown(event: PointerEvent): void {
      if (event.target instanceof Node && rootEl?.contains(event.target)) return;
      open = false;
    }
    window.addEventListener("pointerdown", handlePointerDown, true);
    return () => window.removeEventListener("pointerdown", handlePointerDown, true);
  });
</script>

<svelte:window onkeydown={handleKeydown} />

<div bind:this={rootEl} class="relative inline-flex">
  <button
    type="button"
    onclick={() => void toggle()}
    aria-haspopup="listbox"
    aria-expanded={open}
    aria-label={t("vaultSetup.languageSelectorLabel")}
    class="inline-flex h-8 max-w-full items-center gap-2 rounded-md bg-transparent px-1 text-sm font-medium text-muted-foreground transition-colors hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
  >
    <Languages size={16} strokeWidth={2} class="shrink-0" />
    <span class="truncate">{triggerLabel}</span>
    <ChevronDown
      size={13}
      strokeWidth={2}
      class={cn("shrink-0 transition-transform", open && "rotate-180")}
    />
  </button>

  {#if open}
    <div
      class="surface-floating absolute left-0 top-full z-20 mt-1 w-floating-lg max-w-[calc(100vw-2rem)] overflow-hidden"
    >
      <div class="px-1.5 pt-1.5">
        <label class="field flex items-center gap-1.5">
          <Search size={13} strokeWidth={1.5} class="shrink-0 text-popover-foreground/60" />
          <input
            bind:this={searchInputEl}
            bind:value={query}
            type="search"
            placeholder={t("vaultSetup.languageSearchPlaceholder")}
            class="field-bare text-popover-foreground"
          />
        </label>
      </div>
      <div role="listbox" use:scrollEdgeFadeAction class="surface-floating-body max-h-56 overflow-y-auto">
        {#if filteredOptions.length === 0}
          <div class="px-2 py-1.5 text-panel-detail text-muted-foreground">
            {t("vaultSetup.noLanguagesFound")}
          </div>
        {:else}
          {#each filteredOptions as option (option.value)}
            {@const isActive = option.value === preference}
            <button
              type="button"
              role="option"
              aria-selected={isActive}
              onclick={() => void selectLanguage(option)}
              class="menu-item justify-between gap-3 text-left text-foreground"
            >
              <span class="min-w-0 truncate font-medium">{option.label}</span>
              {#if isActive}
                <Check size={14} strokeWidth={2.5} class="shrink-0" />
              {/if}
            </button>
          {/each}
        {/if}
      </div>
    </div>
  {/if}
</div>
