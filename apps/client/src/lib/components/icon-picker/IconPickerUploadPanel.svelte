<script lang="ts">
  import type { MaybePromise } from "$lib/utils";
  import ImageIcon from "@lucide/svelte/icons/image";
  import Notice from "$lib/components/ui/Notice.svelte";
  import type { IconPickerAsset } from "./types";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";

  let {
    uploadDraft,
    uploadPreviewUrl,
    uploadError,
    uploading,
    uploadBodyStyle,
    uploadUrl = $bindable<string>(),
    onChooseFile,
    onDiscardDraft,
    onSelectDraft,
    onDownloadUrl,
    remoteUrlAvailable = true,
  }: {
    uploadDraft: IconPickerAsset | null;
    uploadPreviewUrl: string | null;
    uploadError: string | null;
    uploading: boolean;
    uploadBodyStyle: string;
    uploadUrl: string;
    onChooseFile: () => MaybePromise;
    onDiscardDraft: () => MaybePromise;
    onSelectDraft: () => MaybePromise;
    onDownloadUrl: () => MaybePromise;
    remoteUrlAvailable?: boolean;
  } = $props();

  const { t } = getLocalization();
</script>

<div use:scrollEdgeFadeAction class="min-h-0 space-y-3 overflow-y-auto p-3" style={uploadBodyStyle}>
  {#if uploadDraft}
    <div class="grid gap-3">
      <div class="flex h-36 items-center justify-center rounded-lg bg-muted/45">
        {#if uploadPreviewUrl}
          <img
            src={uploadPreviewUrl}
            alt=""
            class="size-28 rounded-md object-cover shadow-sm"
          />
        {:else}
          <ImageIcon size={48} strokeWidth={1.5} class="text-muted-foreground" />
        {/if}
      </div>
      <div class="flex items-center justify-between gap-2">
        <button
          type="button"
          class="h-8 rounded-floating-item px-2 text-muted-foreground hover:bg-accent hover:text-foreground"
          onclick={() => { void onDiscardDraft(); }}
        >
          {t("common.cancel")}
        </button>
        <button
          type="button"
          class="h-8 rounded-floating-item bg-primary px-3 font-medium text-primary-foreground"
          onclick={() => { void onSelectDraft(); }}
        >
          {t("common.save")}
        </button>
      </div>
    </div>
  {:else}
    <button
      type="button"
      class="flex min-h-16 w-full items-center justify-center gap-2 rounded-floating-item bg-muted/50 text-muted-foreground hover:bg-accent hover:text-foreground"
      disabled={uploading}
      onclick={() => { void onChooseFile(); }}
    >
      <ImageIcon size={17} strokeWidth={1.75} />
      {t("projects.iconPicker.uploadImage")}
    </button>
    <div class="text-center text-panel-detail text-muted-foreground">{t("projects.iconPicker.pasteHint")}</div>
    {#if remoteUrlAvailable}
      <div class="flex gap-2">
        <input
          bind:value={uploadUrl}
          class="field h-8 min-w-0 flex-1 py-0"
          placeholder={t("projects.iconPicker.imageUrl")}
        />
        <button
          type="button"
          class="h-8 rounded-floating-item bg-primary px-2 font-medium text-primary-foreground disabled:cursor-not-allowed disabled:opacity-50"
          disabled={uploading || !uploadUrl.trim()}
          onclick={() => { void onDownloadUrl(); }}
        >
          {t("projects.iconPicker.fetch")}
        </button>
      </div>
    {/if}
  {/if}
  {#if uploadError}
    <Notice class="text-panel-detail" message={uploadError} />
  {/if}
</div>
