<script lang="ts">
  import { onMount, tick } from "svelte";
  import ZoomIn from "@lucide/svelte/icons/zoom-in";
  import ZoomOut from "@lucide/svelte/icons/zoom-out";
  import { profileImageAssetUrl } from "$lib/api/profile-image";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import {
    activateModalFocus,
    activateModalKeyboardLayer,
    trapModalTabKey,
  } from "$lib/modal-focus";
  import { BUILD_PLATFORM_PROFILE, platformHasCapability } from "$lib/platform";
  import {
    clampProfileImageCrop,
    PROFILE_IMAGE_DEFAULT_CROP,
    PROFILE_IMAGE_MAX_ZOOM,
    PROFILE_IMAGE_MIN_ZOOM,
    profileImageCropFromDrag,
    profileImageCropLayout,
    profileImageCropWithZoom,
    type ProfileImageCrop,
  } from "$lib/profile/identity";
  import { getMobileBackStack } from "$lib/stores/mobile-back-stack.svelte";

  let {
    imagePath,
    initialCrop,
    onApply,
    onCancel,
  }: {
    imagePath: string;
    initialCrop: ProfileImageCrop;
    onApply: (crop: ProfileImageCrop) => void;
    onCancel: () => void;
  } = $props();

  const { t } = getLocalization();
  const mobileBackStack = getMobileBackStack();
  const androidSystemBackAvailable = platformHasCapability(BUILD_PLATFORM_PROFILE, "system.android-back");
  const mobileShell = BUILD_PLATFORM_PROFILE.shell === "mobile";
  const MOVE_STEP = 0.02;
  const MOVE_STEP_LARGE = 0.1;
  const ZOOM_STEP = 0.1;
  const WHEEL_ZOOM_SENSITIVITY = 0.0015;

  let element = $state<HTMLDivElement | null>(null);
  let viewport = $state<HTMLButtonElement | null>(null);
  let viewportSide = $state(0);
  let imageUrl = $state<string | null>(null);
  let imageFailed = $state(false);
  let imageSize = $state({ width: 0, height: 0 });
  // svelte-ignore state_referenced_locally
  let crop = $state<ProfileImageCrop>({ ...initialCrop });
  let drag: { pointerId: number; x: number; y: number; crop: ProfileImageCrop } | null = null;
  let dragging = $state(false);
  /** Dismiss only presses that begin outside the dialog, not drags that end there. */
  let pressStartedOutside = false;

  const imageReady = $derived(imageSize.width > 0 && imageSize.height > 0);
  const layout = $derived(profileImageCropLayout(crop, imageSize));

  $effect(() => {
    let active = true;
    imageUrl = null;
    imageFailed = false;
    void profileImageAssetUrl(imagePath)
      .then((url) => {
        if (active) imageUrl = url;
      })
      .catch(() => {
        if (active) imageFailed = true;
      });
    return () => {
      active = false;
    };
  });

  function setZoom(zoom: number): void {
    crop = profileImageCropWithZoom(crop, zoom, imageSize);
  }

  function apply(): void {
    if (!imageReady) return;
    onApply(clampProfileImageCrop(crop, imageSize));
  }

  /** Capture one pointer so the drag continues outside the crop area on mouse and touch. */
  function startDrag(event: PointerEvent): void {
    if (!imageReady || event.button !== 0 || drag) return;
    const target = event.currentTarget;
    if (!(target instanceof HTMLElement)) return;
    event.preventDefault();
    target.focus({ preventScroll: true });
    target.setPointerCapture(event.pointerId);
    drag = { pointerId: event.pointerId, x: event.clientX, y: event.clientY, crop: { ...crop } };
    dragging = true;
  }

  function moveDrag(event: PointerEvent): void {
    if (!drag || drag.pointerId !== event.pointerId || viewportSide <= 0) return;
    crop = profileImageCropFromDrag(
      drag.crop,
      { x: (event.clientX - drag.x) / viewportSide, y: (event.clientY - drag.y) / viewportSide },
      imageSize,
    );
  }

  function stopDrag(event: PointerEvent): void {
    if (!drag || drag.pointerId !== event.pointerId) return;
    if (event.type === "pointerup") moveDrag(event);
    drag = null;
    dragging = false;
    const target = event.currentTarget;
    if (target instanceof HTMLElement && target.hasPointerCapture(event.pointerId)) {
      target.releasePointerCapture(event.pointerId);
    }
  }

  /** Move, zoom, or reset the crop from the keyboard while the crop area has focus. */
  function handleViewportKey(event: KeyboardEvent): boolean {
    if (!imageReady) return false;
    const step = event.shiftKey ? MOVE_STEP_LARGE : MOVE_STEP;
    const offsets: Partial<Record<string, readonly [number, number]>> = {
      ArrowLeft: [-step, 0],
      ArrowRight: [step, 0],
      ArrowUp: [0, -step],
      ArrowDown: [0, step],
    };
    const delta = offsets[event.key];
    if (delta) {
      crop = profileImageCropFromDrag(crop, { x: delta[0], y: delta[1] }, imageSize);
      return true;
    }
    if (event.key === "+" || event.key === "=") {
      setZoom(crop.zoom + ZOOM_STEP);
      return true;
    }
    if (event.key === "-") {
      setZoom(crop.zoom - ZOOM_STEP);
      return true;
    }
    if (event.key === "Home") {
      crop = clampProfileImageCrop(PROFILE_IMAGE_DEFAULT_CROP, imageSize);
      return true;
    }
    return false;
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (event.key === "Tab" && element) {
      trapModalTabKey(element, event);
      return;
    }
    if (event.key === "Escape") {
      event.preventDefault();
      onCancel();
      return;
    }
    if (event.target === viewport && handleViewportKey(event)) {
      event.preventDefault();
      return;
    }
    if (event.key === "Enter" && !(event.target instanceof HTMLButtonElement && event.target !== viewport)) {
      event.preventDefault();
      apply();
    }
  }

  /** Zoom with the wheel or touchpad without scrolling the dialog. */
  function wheelZoom(node: HTMLElement) {
    const handle = (event: WheelEvent): void => {
      if (!imageReady) return;
      event.preventDefault();
      setZoom(crop.zoom * Math.exp(-event.deltaY * WHEEL_ZOOM_SENSITIVITY));
    };
    node.addEventListener("wheel", handle, { passive: false });
    return {
      destroy() {
        node.removeEventListener("wheel", handle);
      },
    };
  }

  onMount(() => {
    const deactivateMobileBack = androidSystemBackAvailable
      ? mobileBackStack.activate({ handle: () => onCancel() })
      : () => undefined;
    const deactivateKeyboard = activateModalKeyboardLayer(handleKeydown);
    let deactivateModalFocus = (): void => undefined;
    void tick().then(() => {
      if (element) deactivateModalFocus = activateModalFocus(element, viewport);
    });
    return () => {
      deactivateMobileBack();
      deactivateKeyboard();
      deactivateModalFocus();
    };
  });
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="fixed z-90 flex items-center justify-center"
  style="left: var(--visual-viewport-offset-left); top: var(--visual-viewport-offset-top); width: var(--visual-viewport-width); height: var(--visual-viewport-height); padding: calc(var(--safe-area-top) + 1rem) calc(var(--safe-area-right) + 1rem) calc(var(--safe-area-bottom) + 1rem) calc(var(--safe-area-left) + 1rem);"
  onpointerdown={(event) => { pressStartedOutside = !element?.contains(event.target as Node | null); }}
  onclick={(event) => {
    event.stopPropagation();
    if (pressStartedOutside) onCancel();
    pressStartedOutside = false;
  }}
>
  <div class="surface-backdrop absolute inset-0"></div>
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    bind:this={element}
    class="surface-dialog relative z-10 flex max-h-full w-full max-w-sm flex-col gap-4 overflow-y-auto px-5 py-5 outline-none"
    style="--foreground: var(--card-foreground);"
    role="dialog"
    aria-modal="true"
    aria-labelledby="profile-image-crop-title"
    aria-describedby="profile-image-crop-hint"
    tabindex="-1"
    onclick={(event) => event.stopPropagation()}
  >
    <div>
      <h2 id="profile-image-crop-title" class="mb-1 text-[1rem] font-semibold text-foreground">
        {t("settings.profileIdentity.cropTitle")}
      </h2>
      <p id="profile-image-crop-hint" class="text-[0.8rem] text-muted-foreground">
        {t("settings.profileIdentity.cropHint")}
      </p>
    </div>

    <button
      bind:this={viewport}
      bind:clientWidth={viewportSide}
      use:wheelZoom
      type="button"
      class="relative mx-auto block aspect-square w-full max-w-56 shrink-0 touch-none overflow-hidden bg-muted select-none focus:outline-none focus-visible:ring-1 focus-visible:ring-ring"
      style:border-radius="var(--chat-participant-avatar-radius, 22%)"
      style:cursor={imageReady ? (dragging ? "grabbing" : "grab") : "default"}
      aria-label={t("settings.profileIdentity.cropArea")}
      aria-disabled={!imageReady}
      data-app-tooltip-disabled="true"
      onpointerdown={startDrag}
      onpointermove={moveDrag}
      onpointerup={stopDrag}
      onpointercancel={stopDrag}
      onlostpointercapture={stopDrag}
    >
      {#if imageUrl && !imageFailed}
        <img
          class="absolute max-w-none"
          class:invisible={!layout}
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
            crop = clampProfileImageCrop(crop, imageSize);
          }}
          onerror={() => (imageFailed = true)}
        />
      {:else if imageFailed}
        <span class="flex size-full items-center justify-center px-3 text-center text-[0.8rem] text-muted-foreground">
          {t("settings.profileIdentity.invalidPicture")}
        </span>
      {/if}
    </button>

    <div class="flex items-center gap-2 text-muted-foreground">
      <ZoomOut size={15} strokeWidth={1.75} class="shrink-0" aria-hidden="true" />
      <input
        type="range"
        class="min-w-0 flex-1 accent-primary disabled:opacity-40"
        min={PROFILE_IMAGE_MIN_ZOOM}
        max={PROFILE_IMAGE_MAX_ZOOM}
        step="0.01"
        value={crop.zoom}
        disabled={!imageReady}
        aria-label={t("settings.profileIdentity.cropZoom")}
        oninput={(event) => setZoom(Number(event.currentTarget.value))}
      />
      <ZoomIn size={15} strokeWidth={1.75} class="shrink-0" aria-hidden="true" />
    </div>

    <div class="flex flex-wrap items-center justify-start gap-2">
      <button
        type="button"
        onclick={onCancel}
        class="min-h-12 rounded-md border border-border bg-card px-3.5 py-2 text-[0.866667rem] font-medium text-foreground transition-colors hover:bg-accent"
      >
        {#if mobileShell}{t("common.cancel")}{:else}{`${t("common.cancel")} (${t("common.escapeKey")})`}{/if}
      </button>
      <button
        type="button"
        onclick={apply}
        disabled={!imageReady}
        class="min-h-12 rounded-md border border-border bg-primary px-3.5 py-2 text-[0.866667rem] font-medium text-primary-foreground transition-colors hover:bg-primary/90 disabled:opacity-55"
      >
        {#if mobileShell}{t("settings.profileIdentity.cropApply")}{:else}{`${t("settings.profileIdentity.cropApply")} (${t("common.enterKey")})`}{/if}
      </button>
    </div>
  </div>
</div>
