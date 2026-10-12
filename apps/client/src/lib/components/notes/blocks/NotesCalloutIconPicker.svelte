<script lang="ts">
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getProjects } from "$lib/stores/projects.svelte";
  import {
    notesPageIconAssetUrl,
    pickNotesPageIconImageFile,
    saveNotesPageIconImageDataUrl,
  } from "$lib/api/notes/page-icons";
  import type { IconPickerAsset, IconPickerUploadAdapter } from "$lib/components/icon-picker/types";
  import {
    notesPageIconFromPickerValue,
    notesPageIconPickerValue,
  } from "$lib/notes/pages/icon-picker";
  import {
    createNotesExternalPageIcon,
    createNotesLocalFilePageIcon,
    type NotesPageIconAssetMetadata,
  } from "$lib/notes/pages/icon";
  import type { NotesIcon } from "$lib/notes/types";
  import SmilePlus from "@lucide/svelte/icons/smile-plus";
  import IconPicker from "$lib/components/icon-picker/IconPicker.svelte";
  import NotesPageIcon from "$lib/components/notes/pages/NotesPageIcon.svelte";

  let { icon, onChange, initiallyOpen = false }: {
    icon: NotesIcon | null;
    onChange: (icon: NotesIcon | null) => Promise<void> | void;
    initiallyOpen?: boolean;
  } = $props();

  const { t } = getLocalization();
  const projects = getProjects();

  function assetMetadata(asset: IconPickerAsset): NotesPageIconAssetMetadata {
    if (!asset.contentType || asset.byteSize === undefined || !asset.sha256) {
      throw new Error("Callout icon asset metadata is incomplete");
    }
    return {
      relativePath: asset.relativePath,
      originalName: asset.originalName,
      contentType: asset.contentType,
      byteSize: asset.byteSize,
      sha256: asset.sha256,
    };
  }

  const uploadAdapter: IconPickerUploadAdapter = {
    pickImageFile: pickNotesPageIconImageFile,
    saveImageDataUrl: saveNotesPageIconImageDataUrl,
    assetUrl: (asset) => notesPageIconAssetUrl(asset.relativePath),
    selectAsset: (asset) => onChange(createNotesLocalFilePageIcon(assetMetadata(asset))),
    selectExternalUrl: (url) => onChange(createNotesExternalPageIcon(url)),
  };
</script>

<IconPicker
  value={notesPageIconPickerValue(icon)}
  ariaLabel={t("notes.changeCalloutIcon")}
  {initiallyOpen}
  panelAlign="start"
  {uploadAdapter}
  onChange={(value) => onChange(notesPageIconFromPickerValue(value, projects.customEmojis))}
>
  {#snippet trigger({ open, toggle, panelId })}
    <button
      class={`notes-callout-icon flex size-6 shrink-0 items-center justify-center rounded hover:bg-foreground/10 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring ${open ? "bg-foreground/10" : ""}`}
      type="button"
      aria-label={t("notes.changeCalloutIcon")}
      aria-haspopup="dialog"
      aria-expanded={open}
      aria-controls={panelId}
      onclick={toggle}
    >
      {#if icon}
        <NotesPageIcon {icon} size={16} class="notes-callout-icon-glyph" />
      {:else}
        <SmilePlus class="notes-callout-icon-glyph size-4 text-muted-foreground" />
      {/if}
    </button>
  {/snippet}
</IconPicker>
