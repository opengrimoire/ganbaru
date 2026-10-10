<script lang="ts">
  import Upload from "@lucide/svelte/icons/upload";
  import X from "@lucide/svelte/icons/x";
  import type { ProjectIconAsset } from "$lib/api/project-icons";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { cn } from "$lib/utils";
  import { portal } from "$lib/utils/portal";
  import ProjectIcon from "$lib/components/projects/ProjectIcon.svelte";

  let {
    rootElement = $bindable<HTMLElement | undefined>(),
    style,
    portalTarget = "body",
    customEmojiDraft,
    customEmojiName = $bindable(""),
    customEmojiError,
    customEmojiSaving,
    onChooseFile,
    onPaste,
    onSave,
    onClose,
  }: {
    rootElement?: HTMLElement;
    style: string;
    /** Where the panel is portaled; a picker nested in another floating surface keeps its panels inside it. */
    portalTarget?: HTMLElement | string;
    customEmojiDraft: ProjectIconAsset | null;
    customEmojiName: string;
    customEmojiError: string | null;
    customEmojiSaving: boolean;
    onChooseFile: () => void | Promise<void>;
    onPaste: (event: ClipboardEvent) => void | Promise<void>;
    onSave: () => void | Promise<void>;
    onClose: () => void;
  } = $props();

  const { t } = getLocalization();
  const canSaveCustomEmoji = $derived(Boolean(customEmojiDraft && customEmojiName.trim()));
</script>

<section
  bind:this={rootElement}
  use:portal={portalTarget}
  class="surface-floating fixed z-90 flex min-h-0 flex-col overflow-hidden p-4"
  {style}
  data-app-floating-surface
  onpaste={onPaste}
>
  <div class="mb-1 text-[0.933333rem] font-semibold text-foreground">{t("projects.iconPicker.addCustomEmoji")}</div>
  <div class="mb-5 text-panel-detail text-muted-foreground">{t("projects.iconPicker.customEmojiDescription")}</div>
  <button
    type="button"
    class="mb-4 flex min-h-16 w-full items-center justify-center gap-2 rounded-floating-item bg-muted/50 text-muted-foreground hover:bg-accent hover:text-foreground"
    onclick={() => { void onChooseFile(); }}
  >
    {#if customEmojiDraft}
      <ProjectIcon name={`asset:${customEmojiDraft.relativePath}`} size={28} />
    {:else}
      <Upload size={17} strokeWidth={1.75} />
    {/if}
    {t("projects.iconPicker.uploadImage")}
  </button>
  <label class="grid gap-1 text-panel-detail text-muted-foreground">
    {t("projects.iconPicker.emojiName")}
    <input
      bind:value={customEmojiName}
      class="field h-9 text-left text-panel text-foreground"
      placeholder={t("projects.iconPicker.emojiNamePlaceholder")}
    />
  </label>
  {#if customEmojiError}
    <div class="mt-2 rounded-floating-item bg-destructive/10 px-2 py-1 text-panel-detail text-destructive">{customEmojiError}</div>
  {/if}
  <div class="mt-5 flex justify-between gap-2">
    <button
      type="button"
      class="h-8 rounded-floating-item px-2 text-muted-foreground hover:bg-accent hover:text-foreground"
      onclick={onClose}
    >
      {t("common.cancel")}
    </button>
    <button
      type="button"
      class={cn(
        "h-8 rounded-floating-item bg-primary px-3 font-medium text-primary-foreground disabled:cursor-not-allowed",
        canSaveCustomEmoji || customEmojiSaving ? "hover:bg-primary/90" : "opacity-50",
      )}
      disabled={!canSaveCustomEmoji || customEmojiSaving}
      onclick={() => { void onSave(); }}
    >
      {t("common.save")}
    </button>
  </div>
  <button
    type="button"
    class="absolute right-2 top-2 flex size-7 items-center justify-center rounded-floating-item text-muted-foreground hover:bg-accent hover:text-foreground"
    aria-label={t("common.close")}
    onclick={onClose}
  >
    <X size={14} strokeWidth={1.75} />
  </button>
</section>
