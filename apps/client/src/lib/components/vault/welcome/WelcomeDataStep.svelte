<script lang="ts">
  import Database from "@lucide/svelte/icons/database";
  import FileBraces from "@lucide/svelte/icons/file-braces";
  import Folder from "@lucide/svelte/icons/folder";
  import TriangleAlert from "@lucide/svelte/icons/triangle-alert";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { DATA_ENTRIES, splitVaultPath, type DataEntry } from "$lib/vault/welcome-data-folder";
  import WelcomeSceneStep from "./WelcomeSceneStep.svelte";

  /**
   * The last welcome screen, where the person chooses where their data lives. Under the text, a still illustration of the
   * default folder names it, shows where it sits, and lists what the app keeps in it, with the development build's
   * warning at its foot. The choices themselves live in the shared navigation, so the default is the primary action
   * there.
   */
  let {
    headline,
    body,
    location,
    locationLabel,
    developmentWarning = null,
  }: {
    headline: string;
    body: string;
    /** The default location's full path. */
    location: string;
    locationLabel: string;
    developmentWarning?: string | null;
  } = $props();

  const { t } = getLocalization();
  const folder = $derived(splitVaultPath(location));

  /** Every Lucide icon shares one component type. */
  type EntryIcon = typeof Folder;

  function entryIcon(entry: DataEntry): EntryIcon {
    if (entry.folder) return Folder;
    return entry.detail === "database" ? Database : FileBraces;
  }
</script>

<WelcomeSceneStep {headline} {body}>
  <div class="flex flex-1 items-center justify-center">
    <div class="w-full max-w-xl overflow-hidden rounded-2xl border border-border bg-foreground/3">
      <div class="flex items-center gap-3.5 px-4 py-4 @min-[560px]:px-5">
        <span class="grid size-11 shrink-0 place-items-center rounded-xl bg-primary/12 text-primary" aria-hidden="true">
          <Folder size={22} strokeWidth={1.7} fill="currentColor" fill-opacity="0.25" />
        </span>
        <div class="min-w-0 flex-1">
          <p class="text-xs text-muted-foreground">{locationLabel}</p>
          <p class="truncate text-base font-semibold text-foreground" title={location}>{folder.name}</p>
          {#if folder.parent}
            <p class="text-xs break-all text-muted-foreground">{folder.parent}</p>
          {/if}
        </div>
      </div>

      <ul class="border-t border-border py-2">
        {#each DATA_ENTRIES as entry (entry.name)}
          {@const Icon = entryIcon(entry)}
          <li class="flex items-start gap-3 px-4 py-2 @min-[560px]:items-center @min-[560px]:px-5">
            <Icon
              size={16}
              strokeWidth={1.7}
              class={["mt-0.5 shrink-0 @min-[560px]:mt-0", entry.folder ? "text-primary" : "text-muted-foreground"]}
              fill={entry.folder ? "currentColor" : "none"}
              fill-opacity={entry.folder ? 0.25 : undefined}
              aria-hidden="true"
            />
            <span class="flex min-w-0 flex-1 flex-col @min-[560px]:flex-row @min-[560px]:items-center @min-[560px]:gap-4">
              <span class="font-mono text-[13px] text-foreground @min-[560px]:w-36 @min-[560px]:shrink-0">{entry.name}</span>
              <span class="text-sm text-muted-foreground">{t(`vaultSetup.data.entries.${entry.detail}`)}</span>
            </span>
          </li>
        {/each}
      </ul>

      {#if developmentWarning}
        <p class="flex gap-2.5 border-t border-warning/20 bg-warning/8 px-4 py-3 text-[13px] leading-5 text-foreground @min-[560px]:px-5">
          <TriangleAlert size={16} strokeWidth={1.8} class="mt-0.5 shrink-0 text-warning" aria-hidden="true" />
          <span>{developmentWarning}</span>
        </p>
      {/if}
    </div>
  </div>
</WelcomeSceneStep>
