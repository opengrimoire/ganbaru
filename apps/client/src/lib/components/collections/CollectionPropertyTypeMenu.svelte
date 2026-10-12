<script lang="ts" generics="T extends string">
  import ArrowLeftRight from "@lucide/svelte/icons/arrow-left-right";
  import Search from "@lucide/svelte/icons/search";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import CollectionMenu from "./CollectionMenu.svelte";
  import CollectionMenuItem from "./CollectionMenuItem.svelte";
  import CollectionMenuSeparator from "./CollectionMenuSeparator.svelte";
  import { COLLECTION_PROPERTY_ICONS, collectionPropertyTypeSections, type CollectionPropertyTypeOption } from "./property-icons";

  /**
   * A "Change type" row that opens a searchable list of property types with the current one checked. Choosing another
   * type is the menu action; choosing the current type changes nothing.
   */
  let { types, current, disabled = false, onChange }: {
    types: readonly CollectionPropertyTypeOption<T>[];
    current: T;
    disabled?: boolean;
    onChange: (type: T) => void;
  } = $props();

  const localization = getLocalization();
  const { t } = localization;
  let search = $state("");
  const sections = $derived(collectionPropertyTypeSections(types, search, localization.locale));
  const currentLabel = $derived(types.find((option) => option.value === current)?.label ?? "");

  /** Choose the first other matching type on Enter, or move into the list with ArrowDown. */
  function searchKeydown(event: KeyboardEvent & { currentTarget: HTMLInputElement }): void {
    if (event.isComposing) return;
    const list = event.currentTarget.closest("[data-collection-property-type-menu]")?.querySelector("[data-collection-property-type-list]");
    if (event.key === "ArrowDown") {
      event.preventDefault();
      list?.querySelector<HTMLButtonElement>("button:not(:disabled)")?.focus({ preventScroll: true });
      return;
    }
    if (event.key !== "Enter") return;
    event.preventDefault();
    list?.querySelector<HTMLButtonElement>("button:not(:disabled):not([aria-pressed='true'])")?.click();
  }
</script>

<CollectionMenu label={t("collections.property.changeType")} kind="properties" icon={ArrowLeftRight} fullWidth dismissOnAction summary={currentLabel} {disabled}
  onOpenChange={(open) => { if (!open) search = ""; }}>
  <div class="grid gap-0" data-collection-property-type-menu>
    <label class="flex min-h-7 items-center gap-1.5 pl-2 text-[0.75rem] text-muted-foreground">
      <Search class="size-3.5 shrink-0" aria-hidden="true" />
      <input class="field-bare h-7 text-foreground" data-collection-menu-initial-focus aria-label={t("collections.property.searchTypes")} placeholder={t("collections.property.searchTypes")}
        bind:value={search} onkeydown={searchKeydown} />
    </label>
    <div class="grid gap-0" data-collection-property-type-list>
      {#each sections as section, index}
        {#if index > 0}<CollectionMenuSeparator />{/if}
        {#each section as option (option.value)}
          <CollectionMenuItem icon={COLLECTION_PROPERTY_ICONS[option.kind]} label={option.label} checked={option.value === current} {disabled}
            onclick={() => { if (option.value !== current) onChange(option.value); }} />
        {/each}
      {:else}
        <p class="px-2 py-1 text-muted-foreground">{t("collections.property.noTypes")}</p>
      {/each}
    </div>
  </div>
</CollectionMenu>
