<script lang="ts">
  import { onMount, tick, type Component } from "svelte";
  import ArrowDown from "@lucide/svelte/icons/arrow-down";
  import ArrowLeft from "@lucide/svelte/icons/arrow-left";
  import ArrowRight from "@lucide/svelte/icons/arrow-right";
  import ArrowUp from "@lucide/svelte/icons/arrow-up";
  import Search from "@lucide/svelte/icons/search";
  import X from "@lucide/svelte/icons/x";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";
  import CalendarScrollbar from "$lib/components/calendar/CalendarScrollbar.svelte";
  import {
    SHORTCUT_GROUPS,
    filterShortcutGroups,
    localizedShortcutGroups,
    shortcutParts,
  } from "./shortcuts";

  const { t } = getLocalization();
  let search = $state("");
  let searchInput: HTMLInputElement | undefined = $state();
  let shortcutsScrollEl: HTMLDivElement | undefined = $state();
  const shortcutGroups = $derived(localizedShortcutGroups(SHORTCUT_GROUPS, t));
  const filteredGroups = $derived(filterShortcutGroups(shortcutGroups, search));
  const hasSearch = $derived(search.trim().length > 0);

  async function focusSearchInput(): Promise<void> {
    await tick();
    searchInput?.focus();
  }

  function arrowIconForKey(key: string): Component | undefined {
    if (key === "Arrow left") return ArrowLeft;
    if (key === "Arrow right") return ArrowRight;
    if (key === "Arrow up") return ArrowUp;
    if (key === "Arrow down") return ArrowDown;
    return undefined;
  }

  onMount(() => {
    void focusSearchInput();
  });
</script>

<div class="flex h-full min-h-0 flex-col gap-4">
  <div class="shrink-0">
    <label class="relative block">
      <span class="sr-only">{t("settings.shortcuts.search")}</span>
      <Search
        size={14}
        strokeWidth={1.8}
        class="pointer-events-none absolute left-2.5 top-1/2 -translate-y-1/2 text-muted-foreground"
        aria-hidden="true"
      />
      <input
        bind:this={searchInput}
        type="search"
        bind:value={search}
        data-shortcuts-search-input="true"
        placeholder={t("settings.shortcuts.search")}
        class="field w-full pl-8 pr-8 text-[0.866667rem] text-foreground"
      />
      {#if hasSearch}
        <button
          type="button"
          aria-label={t("settings.shortcuts.clearSearch")}
          onclick={() => { search = ""; }}
          class="absolute right-1.5 top-1/2 flex h-5 w-5 -translate-y-1/2 items-center justify-center rounded text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
        >
          <X size={12} strokeWidth={2} />
        </button>
      {/if}
    </label>
  </div>

  <div class="relative min-h-0 flex-1">
    <div bind:this={shortcutsScrollEl} use:scrollEdgeFadeAction class="hide-scrollbar h-full overflow-y-auto pr-1">
      {#if filteredGroups.length === 0}
        <div class="px-1 py-6 text-[0.866667rem] text-muted-foreground">
          {t("settings.shortcuts.noResults")}
        </div>
      {/if}

      <div class="flex flex-col pb-3 pt-1">
        {#each filteredGroups as group, index}
          <section
            class={[
              "flex flex-col gap-4",
              index > 0 ? "pt-7" : "",
            ]}
          >
            <h2 class="px-1 text-[0.866667rem] font-semibold text-foreground">{group.title}</h2>
            <div class="flex flex-col divide-y divide-border">
              {#each group.items as item}
                <div
                  class="grid grid-cols-[minmax(0,1fr)_minmax(8rem,13rem)] gap-x-3 gap-y-1 px-1 py-2.5 first:pt-0 max-[560px]:grid-cols-1"
                >
                  <div class="min-w-0 text-[0.866667rem] text-foreground">
                    <div class="min-h-6 leading-6">{item.action}</div>
                    {#if item.context}
                      <div class="text-[0.733333rem] leading-4 text-muted-foreground">{item.context}</div>
                    {/if}
                  </div>
                  <div class="flex min-w-0 flex-col items-start gap-1.5 overflow-hidden">
                    {#each item.keys as key}
                      <span class="flex flex-wrap items-center gap-1">
                        {#each shortcutParts(key) as part, i}
                          {#if i > 0}
                            <span class="text-[0.733333rem] leading-5 text-muted-foreground">+</span>
                          {/if}
                          {@const ArrowIcon = arrowIconForKey(part)}
                          <kbd
                            class="inline-flex min-h-6 items-center justify-center rounded border border-border bg-background px-1.5 py-0.5 font-mono text-[0.733333rem] leading-5 text-foreground shadow-sm"
                            aria-label={ArrowIcon ? part : undefined}
                            title={ArrowIcon ? part : undefined}
                          >
                            {#if ArrowIcon}
                              <ArrowIcon size={13} strokeWidth={2.1} aria-hidden="true" />
                            {:else}
                              {part}
                            {/if}
                          </kbd>
                        {/each}
                      </span>
                    {/each}
                  </div>
                </div>
              {/each}
            </div>
          </section>
          {#if index < filteredGroups.length - 1}
            <div class="h-px shrink-0 scale-y-50 bg-border" aria-hidden="true"></div>
          {/if}
        {/each}
      </div>
    </div>
    <CalendarScrollbar scrollContainer={shortcutsScrollEl} wheelPassthrough />
  </div>
</div>
