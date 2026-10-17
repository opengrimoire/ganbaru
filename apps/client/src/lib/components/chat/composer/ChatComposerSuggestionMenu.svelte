<script lang="ts">
  import Activity from "@lucide/svelte/icons/activity";
  import Box from "@lucide/svelte/icons/box";
  import CircleGauge from "@lucide/svelte/icons/circle-gauge";
  import CommandIcon from "@lucide/svelte/icons/command";
  import Hammer from "@lucide/svelte/icons/hammer";
  import ListChecks from "@lucide/svelte/icons/list-checks";
  import RotateCcw from "@lucide/svelte/icons/rotate-ccw";
  import ScanSearch from "@lucide/svelte/icons/scan-search";
  import Server from "@lucide/svelte/icons/server";
  import ShieldCheck from "@lucide/svelte/icons/shield-check";
  import Sparkles from "@lucide/svelte/icons/sparkles";
  import Target from "@lucide/svelte/icons/target";
  import type {
    ChatPromptCatalogEntry,
    ProjectWorkingFolderPathRead,
  } from "$lib/chat/contracts";
  import { promptEntryDisplayLabel } from "$lib/chat/composer/command-model";
  import Checkbox from "$lib/components/ui/Checkbox.svelte";
  import Loading from "$lib/components/ui/Loading.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";

  let {
    element = $bindable(),
    kind,
    entries,
    selectedIndex,
    loading,
    includeIgnored = $bindable(),
    hasMore,
    onRefreshMentions,
    onChoose,
    onLoadMore,
  }: {
    element?: HTMLDivElement;
    kind: "mention" | "skill" | "command";
    entries: (ProjectWorkingFolderPathRead | ChatPromptCatalogEntry)[];
    selectedIndex: number;
    loading: boolean;
    includeIgnored: boolean;
    hasMore: boolean;
    onRefreshMentions: () => void;
    onChoose: (index: number) => void;
    onLoadMore: () => void;
  } = $props();

  const { t } = getLocalization();

  function promptEntryIcon(entry: ChatPromptCatalogEntry): typeof Activity {
    if (entry.kind === "skill") return Sparkles;
    switch (entry.value.toLowerCase()) {
      case "/review":
      case "/changes": return ScanSearch;
      case "/compact":
      case "/context": return CircleGauge;
      case "/goal": return Target;
      case "/mcp": return Server;
      case "/plan": return ListChecks;
      case "/build": return Hammer;
      case "/model": return Box;
      case "/permissions": return ShieldCheck;
      case "/status":
      case "/usage": return Activity;
      case "/clear": return RotateCcw;
      default: return CommandIcon;
    }
  }
</script>

<div class="composer-menu-surface surface-floating">
  <div
    bind:this={element}
    id="chat-composer-menu"
    class="composer-menu surface-floating-body"
    role="listbox"
    aria-label={kind === "mention"
      ? t("chat.composer.mentionFiles")
      : kind === "skill"
        ? t("chat.composer.skillsMenu")
        : t("chat.composer.commandsMenu")}
    use:scrollEdgeFadeAction
  >
    {#if kind === "mention"}
      <label><Checkbox checked={includeIgnored} onChange={(next) => { includeIgnored = next; onRefreshMentions(); }} />{t("chat.composer.showIgnored")}</label>
    {/if}
    {#if loading}
      <Loading layout="rows" rows={3} />
    {:else if entries.length === 0}
      <p>{t("chat.composer.noMatches")}</p>
    {:else if kind === "mention"}
      {#each entries as entry, index}
        {#if "relativePath" in entry}<button id={`chat-composer-option-${index}`} data-menu-index={index} type="button" class="menu-item" data-highlighted={index === selectedIndex ? "" : undefined} role="option" aria-selected={index === selectedIndex} onpointerdown={(event) => event.preventDefault()} onclick={() => onChoose(index)}><span class="mention-copy"><strong>{entry.displayName}</strong><small>{entry.relativePath}{#if entry.ignored} · {t("chat.composer.ignored")}{/if}</small></span></button>{/if}
      {/each}
      {#if hasMore}<button type="button" class="menu-item" onclick={onLoadMore}>{t("chat.composer.loadMore")}</button>{/if}
    {:else}
      {#each entries as entry, index}
        {#if "value" in entry}
          {@const EntryIcon = promptEntryIcon(entry)}
          <button id={`chat-composer-option-${index}`} data-menu-index={index} type="button" class="menu-item prompt-option" data-highlighted={index === selectedIndex ? "" : undefined} role="option" aria-selected={index === selectedIndex} onpointerdown={(event) => event.preventDefault()} onclick={() => onChoose(index)}>
            <EntryIcon size={15} strokeWidth={1.8} />
            <span class="prompt-option-copy">
              <strong>{promptEntryDisplayLabel(entry)}</strong>
              {#if entry.description}<small>{entry.description}</small>{/if}
              {#if entry.stale}<small class="stale-command">{t("chat.composer.staleEntry")}</small>{/if}
            </span>
          </button>
        {/if}
      {/each}
    {/if}
  </div>
</div>

<style>
  .composer-menu-surface { position: absolute; inset-inline: 0; bottom: calc(100% + 0.35rem); z-index: 25; display: flex; max-height: min(20rem, 55vh); flex-direction: column; overflow: hidden; }
  .composer-menu { position: relative; min-height: 0; flex: 1 1 auto; overflow: auto; overscroll-behavior: contain; scrollbar-gutter: stable; }
  .composer-menu > label, .composer-menu > p { display: flex; align-items: center; gap: 0.35rem; padding: 0.35rem 0.5rem; color: var(--muted-foreground); font-size: var(--panel-detail-font-size); }
  .mention-copy { display: grid; min-width: 0; flex: 1 1 0%; }
  .composer-menu strong, .composer-menu small { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .composer-menu strong { font-weight: 500; }
  .composer-menu small { color: var(--muted-foreground); font-size: var(--panel-detail-font-size); }
  .prompt-option > :global(svg) { flex: 0 0 auto; color: var(--muted-foreground); }
  .prompt-option-copy { display: flex; min-width: 0; flex: 1; align-items: baseline; gap: 0.45rem; }
  .prompt-option-copy strong, .prompt-option-copy small { display: block; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .prompt-option-copy strong { flex: 0 0 auto; color: var(--foreground); }
  .prompt-option-copy small { flex: 1; }
  .prompt-option-copy .stale-command { flex: 0 1 auto; color: var(--status-tentative); }
  @container chat-composer (max-width: 390px) { .prompt-option-copy small { display: none; } }
</style>
