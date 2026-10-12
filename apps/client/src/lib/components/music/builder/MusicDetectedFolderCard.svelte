<script lang="ts">
  import FolderOpen from "@lucide/svelte/icons/folder-open";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import type { MusicSourcesController } from "$lib/music/sources/controller.svelte";

  let { controller, onAdded = () => undefined }: {
    controller: MusicSourcesController;
    onAdded?: () => void;
  } = $props();

  const { t } = getLocalization();
  let error = $state<string | null>(null);
  const selection = $derived(controller.detectedDefaultFolder);

  async function addFolder(): Promise<void> {
    error = null;
    try {
      const collectionId = await controller.addDetectedDefaultFolder();
      if (collectionId) onAdded();
    } catch (cause) {
      error = cause instanceof Error ? cause.message : String(cause);
    }
  }
</script>

{#if selection}
  <section class="detected-folder-card w-full max-w-xl rounded-2xl border border-primary/25 bg-card/75 p-3.5 text-left shadow-sm" aria-labelledby="detected-music-folder-title">
    <div class="flex items-start gap-3">
      <span class="grid h-11 w-11 shrink-0 place-items-center rounded-xl bg-primary/10 text-primary"><FolderOpen size={20} strokeWidth={1.5} /></span>
      <div class="min-w-0 flex-1">
        <h2 id="detected-music-folder-title" class="text-sm font-semibold">{t("music.builder.detectedFolderTitle")}</h2>
        <p class="mt-1 text-[0.7rem] leading-relaxed text-muted-foreground">{t("music.builder.detectedFolderDescription", selection.tracks.length)}</p>
        <p class="mt-1.5 truncate rounded-lg bg-secondary/60 px-2 py-1.5 text-[0.65rem] text-secondary-foreground" title={selection.folderPath}>{selection.displayName ?? selection.folderPath}</p>
        {#if selection.truncated}<p class="mt-1.5 text-[0.63rem] leading-relaxed text-muted-foreground">{t("music.builder.previewTruncated")}</p>{/if}
      </div>
    </div>
    {#if error}<Notice class="mt-2 text-[0.68rem]" message={error} />{/if}
    <div class="mt-3 flex flex-wrap justify-end gap-2">
      <button type="button" onclick={() => controller.dismissDefaultMusicFolder()} disabled={controller.addingDefaultFolder} class="h-8 rounded-lg bg-secondary px-3 text-xs font-medium text-secondary-foreground hover:bg-accent hover:text-accent-foreground disabled:opacity-45">{t("music.builder.notNow")}</button>
      <button type="button" onclick={() => { void addFolder(); }} disabled={controller.addingDefaultFolder} class="inline-flex h-8 items-center gap-1.5 rounded-lg bg-primary px-3 text-xs font-semibold text-primary-foreground hover:bg-primary/90 disabled:opacity-45">
        {#if controller.addingDefaultFolder}<LoaderCircle class="animate-spin motion-reduce:animate-none" size={13} />{/if}
        {t("music.builder.addDetectedFolder")}
      </button>
    </div>
  </section>
{/if}

<style>
  .detected-folder-card { background-image: radial-gradient(circle at top right, color-mix(in srgb, var(--primary) 10%, transparent), transparent 42%); }
</style>
