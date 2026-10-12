<script lang="ts">
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import type { MusicSourceCollection } from "$lib/music/library/contracts";
  import type { MusicSourcesController } from "$lib/music/sources/controller.svelte";
  import MusicBuilderDialog from "./MusicBuilderDialog.svelte";

  let { controller, collection, onClose, onRemoved }: { controller: MusicSourcesController; collection: MusicSourceCollection; onClose: () => void; onRemoved: () => void } = $props();
  const { t } = getLocalization();
  let choice = $state<"binding" | "source" | "orphans">("binding");
  let saving = $state(false);
  let error = $state<string | null>(null);
  const orphanedItemCount = $derived(controller.removalImpact?.orphanedItemCount ?? 0);

  const consequence = $derived(
    choice === "binding"
      ? t("music.builder.sourceRemovalBindingImpact")
      : choice === "source"
        ? t("music.builder.sourceRemovalSourceImpact")
        : t("music.builder.sourceRemovalOrphanImpact", orphanedItemCount),
  );

  const actionLabel = $derived(
    saving
      ? t("music.builder.saving")
      : choice === "binding"
        ? t("music.builder.removeBinding")
        : choice === "source"
          ? t("music.builder.stopDiscovery")
          : t("music.builder.removeOrphans"),
  );

  $effect(() => {
    if (collection.kind !== "local-root" && choice === "binding") choice = "source";
  });

  async function confirm(): Promise<void> {
    saving = true;
    error = null;
    try {
      if (choice === "binding" && collection.localRootId) await controller.forgetBinding(collection.localRootId);
      else await controller.confirmRemoval(collection, choice === "orphans");
      onRemoved();
    } catch (cause) { error = cause instanceof Error ? cause.message : String(cause); }
    finally { saving = false; }
  }
</script>

<MusicBuilderDialog
  title={t("music.builder.sourceRemovalTitle")}
  titleId="music-source-removal-title"
  description={collection.name}
  size="small"
  role="alertdialog"
  dismissDisabled={saving}
  onDismiss={onClose}
>
  <fieldset>
    <legend class="sr-only">{t("music.builder.sourceRemovalTitle")}</legend>
    {#if collection.kind === "local-root"}
      <label class="choice-row">
        <input type="radio" bind:group={choice} value="binding" class="choice-input" />
        <span class="choice-control" aria-hidden="true"></span>
        <span class="font-medium">{t("music.builder.removeBinding")}</span>
      </label>
    {/if}
    <label class="choice-row">
      <input type="radio" bind:group={choice} value="source" class="choice-input" />
      <span class="choice-control" aria-hidden="true"></span>
      <span class="font-medium">{t("music.builder.stopDiscovery")}</span>
    </label>
    <label class="choice-row">
      <input type="radio" bind:group={choice} value="orphans" class="choice-input" />
      <span class="choice-control" aria-hidden="true"></span>
      <span class="font-medium">{t("music.builder.removeOrphans")}</span>
    </label>
  </fieldset>
  <p class="mt-4 text-xs leading-relaxed text-muted-foreground"><span class="font-medium text-foreground">{t("music.builder.sourceRemovalFilesStay")}</span> {consequence}</p>
  {#if error}<Notice class="mt-3 text-sm" message={error} />{/if}
  {#snippet footer()}
    <button type="button" onclick={onClose} disabled={saving} class="min-h-10 rounded-md border border-border bg-card px-3.5 py-2 text-sm font-medium hover:bg-accent disabled:opacity-50">{t("music.builder.cancel")}</button>
    <button type="button" onclick={() => { void confirm(); }} disabled={saving} class:destructive-action={choice !== "binding"} class:primary-action={choice === "binding"} class="min-h-10 rounded-md px-3.5 py-2 text-sm font-medium disabled:opacity-50">{actionLabel}</button>
  {/snippet}
</MusicBuilderDialog>

<style>
  .choice-row { position: relative; display: grid; min-height: 2.75rem; cursor: pointer; grid-template-columns: 1.1rem minmax(0, 1fr); align-items: center; column-gap: 0.75rem; font-size: calc(0.8rem * var(--type-scale)); }
  .choice-input { position: absolute; height: 1px; width: 1px; overflow: hidden; opacity: 0; }
  .choice-control { display: grid; height: 1rem; width: 1rem; place-items: center; border: 1.5px solid color-mix(in srgb, var(--foreground) 28%, transparent); border-radius: 999px; }
  .choice-control::after { height: 0.45rem; width: 0.45rem; border-radius: 999px; background: var(--foreground); content: ""; opacity: 0; transform: scale(0.5); transition: opacity 100ms ease, transform 100ms ease; }
  .choice-input:checked + .choice-control { border-color: var(--foreground); }
  .choice-input:checked + .choice-control::after { opacity: 1; transform: scale(1); }
  .choice-input:focus-visible + .choice-control { outline: 2px solid var(--ring); outline-offset: 2px; }
  .primary-action { background: var(--primary); color: var(--primary-foreground); }
  .primary-action:hover { background: color-mix(in srgb, var(--primary) 90%, black); }
  .destructive-action { background: var(--destructive); color: var(--destructive-foreground); }
  .destructive-action:hover { background: color-mix(in srgb, var(--destructive) 90%, black); }
  @media (prefers-reduced-motion: reduce) { .choice-control::after { transition: none; } }
</style>
