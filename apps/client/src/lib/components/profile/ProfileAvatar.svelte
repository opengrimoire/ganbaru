<script lang="ts">
  import { profileImageAssetUrl } from "$lib/api/profile-image";
  import type { EventColor } from "$lib/calendar/types";
  import { getEventColor } from "$lib/calendar/utils";
  import {
    PROFILE_IMAGE_DEFAULT_CROP,
    profileImageCropLayout,
    profileInitials,
    type ProfileImageCrop,
  } from "$lib/profile/identity";
  import { getTheme } from "$lib/stores/theme.svelte";

  let {
    displayName,
    imagePath = null,
    crop = PROFILE_IMAGE_DEFAULT_CROP,
    color = null,
    size = 36,
  }: {
    displayName: string;
    imagePath?: string | null;
    /** Square crop of the picture; the default fills the avatar from the image center. */
    crop?: ProfileImageCrop;
    /** Theme palette slot behind the initials; null keeps the neutral surface. */
    color?: EventColor | null;
    size?: number;
  } = $props();

  const theme = getTheme();
  let imageUrl = $state<string | null>(null);
  let imageSize = $state({ width: 0, height: 0 });
  let imageRequestId = 0;
  const initials = $derived(profileInitials(displayName));
  const colorEntry = $derived(color === null ? null : getEventColor(color, theme.current));
  const layout = $derived(profileImageCropLayout(crop, imageSize));

  $effect(() => {
    const path = imagePath;
    const requestId = ++imageRequestId;
    imageUrl = null;
    imageSize = { width: 0, height: 0 };
    if (!path) return;
    void profileImageAssetUrl(path)
      .then((url) => {
        if (requestId === imageRequestId) imageUrl = url;
      })
      .catch(() => {
        if (requestId === imageRequestId) imageUrl = null;
      });
  });
</script>

<span
  class="profile-avatar"
  class:has-image={imageUrl !== null}
  style:width={`${size}px`}
  style:height={`${size}px`}
  style:--profile-avatar-font-size={`${Math.max(10, Math.round(size * 0.34))}px`}
  style:--profile-avatar-bg={colorEntry?.bg}
  style:--profile-avatar-text={colorEntry?.text}
  role="img"
  aria-label={displayName}
>
  {#if imageUrl}
    <img
      class:cropped={layout !== null}
      style:width={layout ? `${layout.width * 100}%` : undefined}
      style:height={layout ? `${layout.height * 100}%` : undefined}
      style:left={layout ? `${layout.left * 100}%` : undefined}
      style:top={layout ? `${layout.top * 100}%` : undefined}
      src={imageUrl}
      alt=""
      draggable="false"
      onload={(event) => {
        const image = event.currentTarget;
        if (!(image instanceof HTMLImageElement)) return;
        imageSize = { width: image.naturalWidth, height: image.naturalHeight };
      }}
    />
  {:else}
    <span aria-hidden="true">{initials}</span>
  {/if}
</span>

<style>
  .profile-avatar {
    position: relative;
    display: inline-grid;
    flex: 0 0 auto;
    place-items: center;
    overflow: hidden;
    border: 1px solid color-mix(in srgb, var(--border) 72%, transparent);
    border-radius: var(--chat-participant-avatar-radius, 22%);
    background: var(--profile-avatar-bg, color-mix(in srgb, var(--foreground) 10%, var(--card)));
    color: var(--profile-avatar-text, var(--foreground));
    font-size: var(--profile-avatar-font-size);
    font-weight: 700;
    line-height: 1;
  }

  /* A picture paints no color, so subpixel gaps at the rounded edge show the surface instead of the palette slot. */
  .profile-avatar.has-image {
    background: transparent;
  }

  .profile-avatar img {
    display: block;
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .profile-avatar img.cropped {
    position: absolute;
    max-width: none;
    object-fit: fill;
  }
</style>
