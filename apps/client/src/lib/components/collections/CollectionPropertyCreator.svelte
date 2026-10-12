<script lang="ts" generics="T extends string">
  import Search from "@lucide/svelte/icons/search";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { COLLECTION_PROPERTY_ICONS, collectionPropertyTypeSections, type CollectionPropertyTypeOption } from "./property-icons";

  /**
   * Lists the property types a new property can have, under a search field that filters them. Choosing a type is the menu action.
   * The property draft header owns the name and icon, and keeps initial focus.
   */
  let { types, pending = false, onCreate }: {
    types: readonly CollectionPropertyTypeOption<T>[];
    pending?: boolean;
    onCreate: (type: T) => void;
  } = $props();

  const localization = getLocalization();
  const { t } = localization;
  let search = $state("");
  const sections = $derived(collectionPropertyTypeSections(types, search, localization.locale));

  /** Choose the first matching type when Enter is pressed in the search field, through its button so the menu treats it as an action. */
  function createFirstMatch(event: KeyboardEvent & { currentTarget: HTMLInputElement }): void {
    event.stopPropagation();
    if (event.key !== "Enter" || event.isComposing || pending) return;
    event.preventDefault();
    event.currentTarget.closest("[data-collection-property-creator]")?.querySelector<HTMLButtonElement>("[data-property-type]:not(:disabled)")?.click();
  }
</script>

<div class="grid gap-1" data-collection-property-creator>
  <label class="flex min-h-7 items-center gap-1.5 pl-2 text-[0.75rem] text-muted-foreground">
    <Search class="size-3.5 shrink-0" aria-hidden="true" />
    <input class="field-bare h-7 text-foreground" aria-label={t("collections.property.searchTypes")} placeholder={t("collections.property.searchTypes")}
      bind:value={search} onkeydown={createFirstMatch} />
  </label>
  {#each sections as section, index}
    {#if index > 0}<div class="mx-1 my-1 border-t border-border"></div>{/if}
    <div class="grid grid-cols-2 gap-0.5">
      {#each section as option (option.value)}
        {@const Icon = COLLECTION_PROPERTY_ICONS[option.kind]}
        <button type="button" class="flex min-h-8 min-w-0 items-center gap-2 rounded-md px-2 text-left text-foreground hover:bg-accent" data-property-type={option.value}
          disabled={pending} onclick={() => onCreate(option.value)}>
          <Icon class="size-3.5 shrink-0 text-muted-foreground" strokeWidth={1.75} aria-hidden="true" /><span class="truncate">{option.label}</span>
        </button>
      {/each}
    </div>
  {:else}
    <p class="px-2 py-1 text-muted-foreground">{t("collections.property.noTypes")}</p>
  {/each}
</div>
