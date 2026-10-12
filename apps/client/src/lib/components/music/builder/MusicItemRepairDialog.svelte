<script lang="ts">
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import type { MusicSourcesController } from "$lib/music/sources/controller.svelte";
  import { formatMusicDuration } from "$lib/music/builder/presentation";
  import Checkbox from "$lib/components/ui/Checkbox.svelte";
  import MusicBuilderDialog from "./MusicBuilderDialog.svelte";

  let { controller, itemId, onClose, onRepaired }: { controller: MusicSourcesController; itemId: string; onClose: () => void; onRepaired: () => void } = $props();
  const { t } = getLocalization();
  let busy = $state(false);
  let acceptedWeak = $state(false);
  let error = $state<string | null>(null);

  function strengthLabel(): string {
    if (controller.itemRepairPreview?.matchStrength === "exact") return t("music.builder.identityExact");
    if (controller.itemRepairPreview?.matchStrength === "likely") return t("music.builder.identityLikely");
    return t("music.builder.identityWeak");
  }

  async function choose(): Promise<void> {
    busy = true;
    error = null;
    acceptedWeak = false;
    try { await controller.chooseItemRepair(itemId); }
    catch (cause) { error = cause instanceof Error ? cause.message : String(cause); }
    finally { busy = false; }
  }

  async function apply(): Promise<void> {
    busy = true;
    error = null;
    try { await controller.applyItemRepair(acceptedWeak); onRepaired(); }
    catch (cause) { error = cause instanceof Error ? cause.message : String(cause); }
    finally { busy = false; }
  }

  async function undo(): Promise<void> {
    busy = true;
    try { await controller.undoItemRepair(); }
    catch (cause) { error = cause instanceof Error ? cause.message : String(cause); }
    finally { busy = false; }
  }
</script>

<MusicBuilderDialog
  title={t("music.builder.repairItemLocation")}
  titleId="music-item-repair-title"
  description={t("music.builder.replacementFileDescription")}
  size="small"
  dismissDisabled={busy}
  onDismiss={onClose}
>
    <div>
      {#if controller.itemRepairApplied}
        <div class="py-8 text-center"><h3 class="text-sm font-semibold">{t("music.builder.locationRepaired")}</h3><button type="button" disabled={busy} onclick={() => { void undo(); }} class="mt-4 min-h-10 rounded-md border border-border px-3.5 py-2 text-sm font-medium hover:bg-accent disabled:opacity-50">{t("music.builder.undoRepair")}</button></div>
      {:else if busy && !controller.itemRepairPreview}
        <div class="grid min-h-40 place-items-center"><LoaderCircle class="animate-spin text-muted-foreground motion-reduce:animate-none" size={22} /></div>
      {:else if controller.itemRepairPreview}
        <div class="border-y border-border py-3"><strong class="block truncate text-sm">{controller.itemRepairPreview.title}</strong><span class="mt-1 block truncate text-xs text-muted-foreground">{controller.itemRepairPreview.artist || controller.itemRepairPreview.relativePath}</span><span class="mt-1 block text-xs text-muted-foreground">{formatMusicDuration(controller.itemRepairPreview.durationMs)} · {Math.round(controller.itemRepairPreview.fileSizeBytes / 1024 / 1024 * 10) / 10} MB</span><div class:weak={controller.itemRepairPreview.matchStrength === "weak"} class="match-summary"><strong>{strengthLabel()}</strong>{#each controller.itemRepairPreview.reasons as reason}<p>{reason}</p>{/each}</div></div>
        {#if controller.itemRepairPreview.matchStrength === "weak"}<label class="mt-4 flex items-start gap-2 text-sm leading-relaxed"><Checkbox bind:checked={acceptedWeak} class="mt-1" /><span><strong class="block text-destructive">{t("music.builder.weakMatchWarning")}</strong><span class="mt-1 block text-muted-foreground">{t("music.builder.acceptWeakMatch")}</span></span></label>{/if}
        <button type="button" onclick={() => { void choose(); }} class="mt-4 text-sm font-medium text-primary hover:underline">{t("music.builder.chooseAnotherFile")}</button>
      {:else}
        <div class="py-8 text-center"><button type="button" onclick={() => { void choose(); }} class="min-h-10 rounded-md bg-primary px-4 py-2 text-sm font-medium text-primary-foreground hover:bg-primary/90">{t("music.builder.chooseReplacementFile")}</button></div>
      {/if}
      {#if error}<Notice class="mt-3 text-sm" message={error} />{/if}
    </div>
    {#snippet footer()}
      <button type="button" onclick={onClose} disabled={busy} class="min-h-10 rounded-md border border-border bg-card px-3.5 py-2 text-sm font-medium hover:bg-accent disabled:opacity-50">{t("music.builder.close")}</button>
      {#if controller.itemRepairPreview && !controller.itemRepairApplied}<button type="button" onclick={() => { void apply(); }} disabled={busy || (controller.itemRepairPreview.matchStrength === "weak" && !acceptedWeak)} class="min-h-10 rounded-md border border-border bg-primary px-3.5 py-2 text-sm font-medium text-primary-foreground hover:bg-primary/90 disabled:opacity-50">{t("music.builder.bindLocation")}</button>{/if}
    {/snippet}
</MusicBuilderDialog>

<style>
  .match-summary { margin-top: 0.75rem; color: var(--muted-foreground); }
  .match-summary strong { display: block; color: var(--foreground); font-size: calc(0.8rem * var(--type-scale)); }
  .match-summary p { margin-top: 0.2rem; font-size: calc(0.72rem * var(--type-scale)); line-height: 1.4; }
  .match-summary.weak { color: var(--destructive); }
</style>
