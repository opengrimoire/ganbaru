<script lang="ts">
  import { notesPageIconAssetUrl } from "$lib/api/notes/page-icons";
  import { projectIconAssetUrl } from "$lib/api/project-icons";
  import {
    isNotesPageIconAssetPath,
    isProjectIconAssetPath,
    notesPageIconAssetPath,
    notesPageIconExternalUrl,
    notesPageNativeIconColor,
  } from "$lib/notes/pages/icon";
  import type { NotesPageIcon } from "$lib/notes/types";
  import type { ProjectLucideIconNode } from "$lib/projects/icons/lucide-catalog.generated";
  import { BUILD_PLATFORM_PROFILE, platformHasCapability } from "$lib/platform";
  import FileText from "@lucide/svelte/icons/file-text";
  import LucideNodeIcon from "$lib/components/icon-picker/LucideNodeIcon.svelte";

  let {
    icon,
    size = 18,
    strokeWidth = 1.75,
    emojiScale = 1,
    class: className = "",
  }: {
    icon: NotesPageIcon | null;
    size?: number;
    strokeWidth?: number;
    emojiScale?: number;
    class?: string;
  } = $props();

  let assetUrl = $state<string | null>(null);
  let lucideNode = $state<readonly ProjectLucideIconNode[] | null>(null);
  let assetRequestId = 0;
  let lucideRequestId = 0;

  const assetPath = $derived(notesPageIconAssetPath(icon));
  const remoteImageUrlsAvailable = platformHasCapability(
    BUILD_PLATFORM_PROFILE,
    "notes.external-image-references",
  );
  const externalUrl = $derived(remoteImageUrlsAvailable ? notesPageIconExternalUrl(icon) : null);
  const nativeName = $derived(icon?.type === "icon" ? icon.icon.name : null);
  const nativeColor = $derived(icon?.type === "icon" ? notesPageNativeIconColor(icon.icon.color) : undefined);
  const nativeStyle = $derived(nativeColor ? `color: ${nativeColor};` : undefined);
  const emojiFontSize = $derived(Math.max(1, size * emojiScale));

  $effect(() => {
    const currentPath = assetPath;
    const requestId = ++assetRequestId;
    assetUrl = null;
    if (!currentPath) return;
    const loadAsset = isNotesPageIconAssetPath(currentPath)
      ? notesPageIconAssetUrl
      : isProjectIconAssetPath(currentPath)
        ? projectIconAssetUrl
        : null;
    if (!loadAsset) return;
    void loadAsset(currentPath)
      .then((url) => {
        if (requestId === assetRequestId) assetUrl = url;
      })
      .catch(() => {
        if (requestId === assetRequestId) assetUrl = null;
      });
  });

  $effect(() => {
    const currentName = nativeName;
    const requestId = ++lucideRequestId;
    lucideNode = null;
    if (!currentName) return;
    void import("$lib/projects/icons/lucide-catalog.generated")
      .then((catalog) => {
        if (requestId !== lucideRequestId) return;
        lucideNode = catalog.PROJECT_LUCIDE_ICONS.find((item) => item.slug === currentName)?.iconNode ?? null;
      })
      .catch(() => {
        if (requestId === lucideRequestId) lucideNode = null;
      });
  });
</script>

{#if icon?.type === "emoji"}
  <span
    class={className}
    style={`display: inline-flex; width: ${size}px; height: ${size}px; align-items: center; justify-content: center; overflow: visible; font-size: ${emojiFontSize}px; line-height: 1;`}
    aria-hidden="true"
  >
    {icon.emoji}
  </span>
{:else if assetUrl}
  <img
    src={assetUrl}
    alt=""
    class={className}
    style={`width: ${size}px; height: ${size}px; object-fit: cover; border-radius: 0.25rem;`}
  />
{:else if externalUrl}
  <img
    src={externalUrl}
    alt=""
    class={className}
    style={`width: ${size}px; height: ${size}px; object-fit: cover; border-radius: 0.25rem;`}
  />
{:else if lucideNode}
  <LucideNodeIcon iconNode={lucideNode} {size} {strokeWidth} class={className} style={nativeStyle} />
{:else}
  <FileText {size} {strokeWidth} class={className} style={nativeStyle} />
{/if}
