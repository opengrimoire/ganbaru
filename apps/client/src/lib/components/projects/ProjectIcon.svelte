<script lang="ts">
  import type { Component } from "svelte";
  import Apple from "@lucide/svelte/icons/apple";
  import Bath from "@lucide/svelte/icons/bath";
  import Bed from "@lucide/svelte/icons/bed";
  import Bike from "@lucide/svelte/icons/bike";
  import BookOpen from "@lucide/svelte/icons/book-open";
  import Clapperboard from "@lucide/svelte/icons/clapperboard";
  import Folder from "@lucide/svelte/icons/folder";
  import GraduationCap from "@lucide/svelte/icons/graduation-cap";
  import Heart from "@lucide/svelte/icons/heart";
  import Pill from "@lucide/svelte/icons/pill";
  import Repeat from "@lucide/svelte/icons/repeat";
  import ShoppingCart from "@lucide/svelte/icons/shopping-cart";
  import Smile from "@lucide/svelte/icons/smile";
  import { projectIconAssetUrl } from "$lib/api/project-icons";
  import { getEventColor } from "$lib/calendar/utils";
  import {
    parseProjectIcon,
    projectIconColorToEventColor,
  } from "$lib/projects/icons/values";
  import type { ProjectLucideIconNode } from "$lib/projects/icons/lucide-catalog.generated";
  import { projectAppIconNode } from "$lib/projects/icons/app-icons";
  import { getProjects } from "$lib/stores/projects.svelte";
  import { getTheme } from "$lib/stores/theme.svelte";
  import LucideNodeIcon from "$lib/components/icon-picker/LucideNodeIcon.svelte";

  let {
    name,
    size = 14,
    strokeWidth = 1.75,
    emojiScale = 1,
    class: className = "",
  }: {
    name?: string;
    size?: number;
    strokeWidth?: number;
    emojiScale?: number;
    class?: string;
  } = $props();

  const projects = getProjects();
  const theme = getTheme();

  const icons: Record<string, Component> = {
    apple: Apple,
    bath: Bath,
    bed: Bed,
    bike: Bike,
    "book-open": BookOpen,
    clapperboard: Clapperboard,
    folder: Folder,
    "graduation-cap": GraduationCap,
    heart: Heart,
    pill: Pill,
    repeat: Repeat,
    "shopping-cart": ShoppingCart,
    smile: Smile,
  };

  let assetUrl = $state<string | null>(null);
  let lucideNode = $state<readonly ProjectLucideIconNode[] | null>(null);
  let assetRequestId = 0;
  let lucideRequestId = 0;

  const parsedIcon = $derived(parseProjectIcon(name));
  const customEmoji = $derived(
    parsedIcon.kind === "custom-emoji"
      ? projects.customEmojis.find((emoji) => emoji.id === parsedIcon.id)
      : undefined,
  );
  const relativeAssetPath = $derived(
    parsedIcon.kind === "asset"
      ? parsedIcon.relativePath
      : parsedIcon.kind === "custom-emoji"
        ? customEmoji?.assetPath
        : undefined,
  );
  const Icon = $derived(
    parsedIcon.kind === "lucide"
      ? icons[parsedIcon.slug] ?? null
      : null,
  );
  const appIconNode = $derived(
    parsedIcon.kind === "lucide"
      ? projectAppIconNode(parsedIcon.slug)
      : null,
  );
  const resolvedLucideNode = $derived(appIconNode ?? lucideNode);
  const eventIconColor = $derived(
    parsedIcon.kind === "lucide"
      ? projectIconColorToEventColor(parsedIcon.color)
      : undefined,
  );
  const iconStyle = $derived(
    parsedIcon.kind === "lucide" && eventIconColor !== undefined
      ? `color: ${getEventColor(eventIconColor, theme.current).bg};`
      : undefined,
  );
  const emojiFontSize = $derived(Math.max(1, size * emojiScale));

  $effect(() => {
    const currentPath = relativeAssetPath;
    const requestId = ++assetRequestId;
    assetUrl = null;
    if (!currentPath) return;
    void projectIconAssetUrl(currentPath)
      .then((url) => {
        if (requestId === assetRequestId) assetUrl = url;
      })
      .catch(() => {
        if (requestId === assetRequestId) assetUrl = null;
      });
  });

  $effect(() => {
    const currentIcon = parsedIcon;
    const requestId = ++lucideRequestId;
    lucideNode = null;
    if (currentIcon.kind !== "lucide" || icons[currentIcon.slug] || projectAppIconNode(currentIcon.slug)) return;
    void import("$lib/projects/icons/lucide-catalog.generated")
      .then((catalog) => {
        if (requestId !== lucideRequestId) return;
        lucideNode = catalog.PROJECT_LUCIDE_ICONS.find((icon) => icon.slug === currentIcon.slug)?.iconNode ?? null;
      })
      .catch(() => {
        if (requestId === lucideRequestId) lucideNode = null;
      });
  });
</script>

{#if parsedIcon.kind === "none"}
  <span class={className} style={`display: inline-block; width: ${size}px; height: ${size}px;`} aria-hidden="true"></span>
{:else if parsedIcon.kind === "emoji"}
  <span
    class={className}
    style={`display: inline-flex; width: ${size}px; height: ${size}px; align-items: center; justify-content: center; overflow: visible; font-size: ${emojiFontSize}px; line-height: 1;`}
    aria-hidden="true"
  >
    {parsedIcon.emoji}
  </span>
{:else if relativeAssetPath && assetUrl}
  <img
    src={assetUrl}
    alt=""
    class={className}
    style={`width: ${size}px; height: ${size}px; object-fit: cover; border-radius: 0.25rem;`}
  />
{:else if Icon}
  <Icon {size} {strokeWidth} class={className} style={iconStyle} />
{:else if resolvedLucideNode}
  <LucideNodeIcon iconNode={resolvedLucideNode} {size} {strokeWidth} class={className} style={iconStyle} />
{:else}
  <Folder {size} {strokeWidth} class={className} style={iconStyle} />
{/if}
