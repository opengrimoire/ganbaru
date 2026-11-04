<script lang="ts">
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import type { MusicSourceCollection } from "$lib/music/library/contracts";
  import type { MusicSourcesController } from "$lib/music/sources/controller.svelte";
  import MusicBuilderDialog from "./MusicBuilderDialog.svelte";

  let { controller, collection, onClose, onApplied }: { controller: MusicSourcesController; collection: MusicSourceCollection; onClose: () => void; onApplied: () => void } = $props();
  const { t } = getLocalization();
  let step = $state<"choose" | "planning" | "review" | "applying">("choose");
  let replacementPath = $state("");
  let error = $state<string | null>(null);
  let decisions = $state<Record<string, string | null | undefined>>({});
  const ambiguousEntries = $derived(controller.relinkEntries.filter((entry) => entry.matchKind === "ambiguous"));
  const everyAmbiguityResolved = $derived(ambiguousEntries.every((entry) => decisions[entry.id] !== undefined));

  async function chooseFolder(): Promise<void> {
    error = null;
    const selected = await controller.chooseLocalFolder();
    if (!selected) return;
    replacementPath = selected.selection.folderPath;
    step = "planning";
    try {
      const plan = await controller.planRelink(collection.localRootId!, replacementPath);
      step = plan.state === "ready" ? "review" : "choose";
    }
    catch (cause) { error = cause instanceof Error ? cause.message : String(cause); step = "choose"; }
  }

  async function apply(): Promise<void> {
    if (!everyAmbiguityResolved) return;
    step = "applying";
    error = null;
    try {
      await controller.applyRelink(Object.entries(decisions).flatMap(([entryId, itemId]) => itemId ? [{ entryId, itemId }] : []));
      onApplied();
    } catch (cause) { error = cause instanceof Error ? cause.message : String(cause); step = "review"; }
  }

  async function close(): Promise<void> {
    if (controller.relinkCancellable) await controller.cancelRelink();
    onClose();
  }
</script>

<MusicBuilderDialog
  title={`${t("music.builder.relinkRoot")}: ${collection.name}`}
  titleId="music-relink-title"
  size="large"
  dismissDisabled={step === "applying"}
  onDismiss={() => { void close(); }}
>
    <div class="relink-scroll">
      {#if step === "choose"}
        <div class="py-8 text-center"><h3 class="text-sm font-semibold">{t("music.builder.relinkChoose")}</h3><p class="mx-auto mt-1 max-w-sm text-sm leading-relaxed text-muted-foreground">{t("music.builder.dataPreserved")}</p><button type="button" onclick={() => { void chooseFolder(); }} class="mt-4 min-h-10 rounded-md bg-primary px-4 py-2 text-sm font-medium text-primary-foreground hover:bg-primary/90">{t("music.builder.chooseFolder")}</button></div>
      {:else if step === "planning"}
        <div class="grid min-h-44 place-items-center"><div class="text-center"><LoaderCircle class="mx-auto animate-spin text-muted-foreground motion-reduce:animate-none" size={23} /><p class="mt-2 text-sm text-muted-foreground">{t("music.builder.refreshing")}</p></div></div>
      {:else if controller.relinkPlan}
        <div class="grid grid-cols-5 divide-x divide-border border-y border-border py-3 max-[480px]:grid-cols-3 max-[480px]:divide-x-0">
          <div class="relink-stat"><strong>{controller.relinkPlan.exactCount}</strong><span>{t("music.builder.exactMatches")}</span></div><div class="relink-stat"><strong>{controller.relinkPlan.likelyCount}</strong><span>{t("music.builder.likelyMatches")}</span></div><div class="relink-stat"><strong>{controller.relinkPlan.ambiguousCount}</strong><span>{t("music.builder.ambiguousMatches")}</span></div><div class="relink-stat"><strong>{controller.relinkPlan.missingCount}</strong><span>{t("music.builder.missing")}</span></div><div class="relink-stat"><strong>{controller.relinkPlan.newCount}</strong><span>{t("music.builder.newMatches")}</span></div>
        </div>
        {#if ambiguousEntries.length > 0}
          <div class="mt-3 divide-y divide-border">
            {#each ambiguousEntries as entry (entry.id)}
              <div class="py-3"><p class="truncate text-sm font-medium">{entry.candidateRelativePath ?? entry.id}</p><p class="mt-0.5 text-xs text-muted-foreground">{t("music.builder.ambiguous")}</p><div class="mt-2 flex flex-wrap gap-1.5">{#each entry.candidateItemIds as itemId}<button type="button" onclick={() => decisions[entry.id] = itemId} class:selected-decision={decisions[entry.id] === itemId} class="decision">{itemId}</button>{/each}<button type="button" onclick={() => decisions[entry.id] = null} class:selected-decision={decisions[entry.id] === null} class="decision">{t("music.builder.missing")}</button></div></div>
            {/each}
          </div>
          {#if !everyAmbiguityResolved}<p class="mt-2 text-xs text-destructive">{t("music.builder.unresolvedAmbiguities")}</p>{/if}
        {:else}<p class="mt-3 text-sm text-muted-foreground">{t("music.builder.dataPreserved")}</p>{/if}
      {/if}
      {#if error}<Notice class="mt-3 text-sm" message={error} />{/if}
    </div>
    {#snippet footer()}
      <button type="button" onclick={() => { void close(); }} disabled={step === "applying"} class="min-h-10 rounded-md border border-border bg-card px-3.5 py-2 text-sm font-medium hover:bg-accent disabled:opacity-50">{t("music.builder.cancel")}</button>
      {#if step === "review" || step === "applying"}<button type="button" onclick={() => { void apply(); }} disabled={!everyAmbiguityResolved || step === "applying"} class="min-h-10 rounded-md border border-border bg-primary px-3.5 py-2 text-sm font-medium text-primary-foreground hover:bg-primary/90 disabled:opacity-50">{t("music.builder.applyRelink")}</button>{/if}
    {/snippet}
</MusicBuilderDialog>

<style>
  .relink-scroll { scrollbar-width: thin; scrollbar-color: color-mix(in srgb, var(--foreground) 18%, transparent) transparent; }
  .relink-stat { display: flex; min-width: 0; flex-direction: column; padding: 0 0.65rem; }
  .relink-stat strong { font-size: calc(0.8rem * var(--type-scale)); font-variant-numeric: tabular-nums; }
  .relink-stat span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--muted-foreground); font-size: calc(0.58rem * var(--type-scale)); }
  .decision { display: inline-flex; min-height: 1.7rem; max-width: 100%; align-items: center; gap: 0.25rem; overflow: hidden; border: 1px solid var(--border); border-radius: 0.55rem; padding-inline: 0.55rem; color: var(--muted-foreground); font-size: calc(0.61rem * var(--type-scale)); text-overflow: ellipsis; white-space: nowrap; }
  .selected-decision { border-color: color-mix(in srgb, var(--primary) 45%, var(--border)); background: color-mix(in srgb, var(--primary) 10%, var(--card)); color: var(--foreground); }
</style>
