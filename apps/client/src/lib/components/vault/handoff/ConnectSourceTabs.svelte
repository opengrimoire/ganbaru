<script lang="ts" generics="Id extends string">
  import { tick } from "svelte";
  import { formatListParts } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { cn } from "$lib/utils";

  /**
   * The places to connect to, written into a heading as a localized "or" list, such as "your phone,
   * another computer, or a server". Each place is a tab that switches the panel below, and the
   * arrow, Home, and End keys move between them as in a tab list.
   */
  let {
    sources,
    active,
    disabled = false,
    idPrefix,
    panelId,
    onChoose,
  }: {
    sources: readonly { id: Id; label: string }[];
    active: Id;
    disabled?: boolean;
    /** Prefix of each tab's id, followed by its source id. */
    idPrefix: string;
    /** Id of the tab panel the tabs control. */
    panelId: string;
    /** Called with the chosen source and whether the keyboard chose it, which keeps focus on the tabs. */
    onChoose: (id: Id, fromKeyboard: boolean) => void;
  } = $props();

  const localization = getLocalization();
  const tabs: HTMLButtonElement[] = $state([]);
  const parts = $derived(
    formatListParts(localization.locale, sources.map((source) => source.label), { type: "disjunction" }),
  );

  /**
   * Moves to another source when a tab has focus and an arrow, Home, or End key is pressed, and
   * answers whether it claimed the key. A modal that captures keys before the tabs calls it itself.
   */
  export function handleKeydown(event: KeyboardEvent): boolean {
    if (disabled || !(event.target instanceof HTMLButtonElement) || !tabs.includes(event.target)) return false;
    const current = sources.findIndex((source) => source.id === active);
    const last = sources.length - 1;
    const next = event.key === "ArrowRight" || event.key === "ArrowDown"
      ? (current + 1) % sources.length
      : event.key === "ArrowLeft" || event.key === "ArrowUp"
        ? (current + last) % sources.length
        : event.key === "Home" ? 0 : event.key === "End" ? last : null;
    if (next === null) return false;
    event.preventDefault();
    onChoose(sources[next].id, true);
    void tick().then(() => tabs[next]?.focus());
    return true;
  }
</script>

<!-- svelte-ignore a11y_interactive_supports_focus -->
<span role="tablist" aria-orientation="horizontal" onkeydown={(event) => { handleKeydown(event); }}>
  {#each parts as part, partIndex (partIndex)}
    {#if part.type === "separator"}
      <span class="font-normal text-muted-foreground">{part.text}</span>
    {:else}
      {@const source = sources[part.index]}
      {@const selected = source.id === active}
      <button
        bind:this={tabs[part.index]}
        type="button"
        role="tab"
        id={`${idPrefix}-${source.id}`}
        aria-selected={selected}
        aria-controls={panelId}
        tabindex={selected ? 0 : -1}
        class={cn(
          "rounded-sm whitespace-nowrap underline decoration-2 underline-offset-6 transition-colors disabled:pointer-events-none",
          selected
            ? "text-foreground decoration-primary"
            : "cursor-pointer text-muted-foreground decoration-muted-foreground/40 decoration-dotted hover:text-foreground hover:decoration-foreground/60",
        )}
        {disabled}
        onclick={() => onChoose(source.id, false)}
      >
        {source.label}
      </button>
    {/if}
  {/each}
</span>
