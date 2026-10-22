<script lang="ts">
  import { onDestroy } from "svelte";
  import Save from "@lucide/svelte/icons/save";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import Pencil from "@lucide/svelte/icons/pencil";
  import { deleteProfileImageFile } from "$lib/api/profile-image";
  import { pickProfileImageFile } from "$lib/api/profile-image-picker";
  import ProfileAvatar from "$lib/components/profile/ProfileAvatar.svelte";
  import ProfileAvatarPanel from "$lib/components/profile/ProfileAvatarPanel.svelte";
  import ProfileImageCropDialog from "$lib/components/profile/ProfileImageCropDialog.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import type { NotesLocalUser } from "$lib/notes/types";
  import type { EventColor } from "$lib/calendar/types";
  import {
    PROFILE_IMAGE_DEFAULT_CROP,
    profileImageCropsEqual,
    type ProfileImageCrop,
  } from "$lib/profile/identity";
  import type { SettingsDraftKind } from "$lib/settings/types";
  import {
    PROFILE_DISPLAY_NAME_FALLBACK,
    PROFILE_DISPLAY_NAME_MAX_CHARS,
    PROFILE_FULL_NAME_MAX_CHARS,
    isProfileImage,
    normalizeProfileDisplayName,
    type ProfileImage,
    normalizeProfileFullName,
  } from "$lib/stores/preference-options";
  import { getNotes } from "$lib/stores/notes.svelte";
  import { getPreferences } from "$lib/stores/preferences.svelte";
  import { cn } from "$lib/utils";
  import { flushConfig } from "$lib/vault/config";
  import { moveTextInputCaretToPointer } from "$lib/utils/text-input-caret";

  let {
    onDraftStateChange = () => {},
  }: {
    /** Reports whether the section holds unsaved identity or picture changes. */
    onDraftStateChange?: (draft: SettingsDraftKind | null) => void;
  } = $props();

  const notes = getNotes();
  const preferences = getPreferences();
  const { t } = getLocalization();
  const PROFILE_AVATAR_SIZE = 40;

  let avatarTrigger = $state<HTMLButtonElement | undefined>();
  let avatarPanelOpen = $state(false);
  /** Open crop editor; `ownsFile` marks a newly picked file that Cancel must delete. */
  let cropEditor = $state<{
    image: ProfileImage;
    crop: ProfileImageCrop;
    fromUpload: boolean;
    ownsFile: boolean;
  } | null>(null);

  let draftDisplayName = $state(preferences.profileDisplayName);
  let draftFullName = $state(preferences.profileFullName);
  let draftImage = $state<ProfileImage | null>(preferences.profileImage);
  const draftImagePath = $derived(draftImage?.path ?? null);
  let draftImageCrop = $state<ProfileImageCrop>({ ...preferences.profileImageCrop });
  let draftUseImage = $state(preferences.profileUseImage);
  let draftColor = $state<EventColor>(preferences.profileColor);
  /** Picked copy in `assets/profile/` that no saved preference references yet; discarding the draft deletes it. */
  let pendingImagePath: string | null = null;
  let initializedDisplayName = $state<string | null>(null);
  let initializedFullName = $state<string | null>(null);
  let saving = $state(false);
  let savingImage = $state(false);
  let saveError = $state<string | null>(null);
  let saved = $state(false);

  const normalizedDisplayName = $derived(normalizeProfileDisplayName(draftDisplayName));
  const normalizedFullName = $derived(normalizeProfileFullName(draftFullName));
  const validationMessage = $derived.by(() => {
    if (!normalizedDisplayName.ok && normalizedDisplayName.reason === "too_long") {
      return t("settings.profileIdentity.nameTooLong", PROFILE_DISPLAY_NAME_MAX_CHARS);
    }
    if (!normalizedDisplayName.ok) return t("settings.profileIdentity.invalidName");
    if (!normalizedFullName.ok && normalizedFullName.reason === "too_long") {
      return t("settings.profileIdentity.fullNameTooLong", PROFILE_FULL_NAME_MAX_CHARS);
    }
    if (!normalizedFullName.ok) return t("settings.profileIdentity.invalidFullName");
    return null;
  });
  const namesValid = $derived(normalizedDisplayName.ok && normalizedFullName.ok);
  const namesChanged = $derived(
    !normalizedDisplayName.ok
      || !normalizedFullName.ok
      || normalizedDisplayName.value !== preferences.profileDisplayName
      || normalizedFullName.value !== preferences.profileFullName,
  );
  const pictureChanged = $derived(
    draftImagePath !== preferences.profileImagePath
      || draftUseImage !== preferences.profileUseImage
      || draftColor !== preferences.profileColor
      || !profileImageCropsEqual(draftImageCrop, preferences.profileImageCrop),
  );
  const hasSaveableChange = $derived(namesValid && (namesChanged || pictureChanged));
  const canSave = $derived(hasSaveableChange && !saving);

  $effect(() => {
    onDraftStateChange(namesChanged || pictureChanged ? "profile" : null);
    return () => onDraftStateChange(null);
  });

  onDestroy(() => {
    const pending = pendingImagePath;
    if (!pending) return;
    void deleteProfileImageFile(pending).catch((error: unknown) => {
      console.error("Failed to delete a discarded profile picture", error);
    });
  });

  $effect(() => {
    const current = preferences.profileDisplayName;
    if (initializedDisplayName === current) return;
    initializedDisplayName = current;
    draftDisplayName = current;
  });

  $effect(() => {
    const current = preferences.profileFullName;
    if (initializedFullName === current) return;
    initializedFullName = current;
    draftFullName = current;
  });

  async function saveIdentity(): Promise<void> {
    saveError = null;
    saved = false;
    const displayName = normalizeProfileDisplayName(draftDisplayName);
    if (!displayName.ok) {
      saveError = displayName.reason === "too_long"
        ? t("settings.profileIdentity.nameTooLong", PROFILE_DISPLAY_NAME_MAX_CHARS)
        : t("settings.profileIdentity.invalidName");
      return;
    }
    const fullName = normalizeProfileFullName(draftFullName);
    if (!fullName.ok) {
      saveError = fullName.reason === "too_long"
        ? t("settings.profileIdentity.fullNameTooLong", PROFILE_FULL_NAME_MAX_CHARS)
        : t("settings.profileIdentity.invalidFullName");
      return;
    }

    const shouldUpdateNotesName = displayName.value !== preferences.profileDisplayName;
    const notesDisplayName = displayName.value || PROFILE_DISPLAY_NAME_FALLBACK;
    saving = true;
    try {
      if (shouldUpdateNotesName) {
        const updated: NotesLocalUser | null = await notes.updateLocalUserDisplayName(notesDisplayName);
        if (!updated) {
          saveError = notes.localUserError ?? t("settings.profileIdentity.syncFailed");
          return;
        }
      }
      if (!preferences.setProfileDisplayName(displayName.value)) {
        saveError = t("settings.profileIdentity.invalidName");
        return;
      }
      if (!preferences.setProfileFullName(fullName.value)) {
        saveError = t("settings.profileIdentity.invalidFullName");
        return;
      }
      draftDisplayName = displayName.value;
      draftFullName = fullName.value;
      if (pictureChanged) await savePicture();
      saved = true;
    } catch (error) {
      saveError = error instanceof Error ? error.message : String(error);
    } finally {
      saving = false;
    }
  }

  /** Store the picture draft, then delete the saved file it replaced. */
  async function savePicture(): Promise<void> {
    const previous = preferences.profileImagePath;
    const imageSaved = draftImagePath === previous
      ? preferences.setProfileImageCrop(draftImageCrop)
      : preferences.setProfileImage(draftImage, draftImageCrop);
    if (!imageSaved || !preferences.setProfileColor(draftColor)) {
      throw new Error(t("settings.profileIdentity.invalidPicture"));
    }
    preferences.setProfileUseImage(draftUseImage);
    draftImageCrop = { ...preferences.profileImageCrop };
    pendingImagePath = null;
    if (previous && previous !== draftImagePath) {
      // Rust refuses to delete the image the saved config names, so the new path must land first.
      await flushConfig();
      await deleteProfileImageFile(previous);
    }
  }

  function reportImageError(error: unknown): void {
    const message = error instanceof Error ? error.message : String(error);
    saveError = t("settings.profileIdentity.pictureUpdateFailed", message);
  }

  /** Delete a picked copy that the draft no longer references. */
  async function discardImageFile(path: string): Promise<void> {
    try {
      await deleteProfileImageFile(path);
    } catch (error) {
      reportImageError(error);
    }
  }

  function markPictureEdited(): void {
    saveError = null;
    saved = false;
  }

  /** Pick a picture, then crop it before it enters the draft. */
  async function chooseProfileImage(): Promise<void> {
    if (savingImage || saving) return;
    markPictureEdited();
    savingImage = true;
    try {
      const selected = await pickProfileImageFile(t("settings.profileIdentity.picturePickerTitle"));
      if (!selected) return;
      const image = { path: selected.relativePath, byteSize: selected.byteSize };
      const path = image.path;
      const ownsFile = path !== preferences.profileImagePath && path !== pendingImagePath;
      if (!isProfileImage(image)) {
        if (ownsFile) await deleteProfileImageFile(path);
        throw new Error(t("settings.profileIdentity.invalidPicture"));
      }
      avatarPanelOpen = false;
      cropEditor = {
        image,
        crop: path === draftImagePath ? { ...draftImageCrop } : { ...PROFILE_IMAGE_DEFAULT_CROP },
        fromUpload: true,
        ownsFile,
      };
    } catch (error) {
      reportImageError(error);
    } finally {
      savingImage = false;
    }
  }

  function adjustProfileImage(): void {
    const image = draftImage;
    avatarPanelOpen = false;
    if (!image) return;
    cropEditor = { image, crop: { ...draftImageCrop }, fromUpload: false, ownsFile: false };
  }

  function closeCropEditor(): void {
    cropEditor = null;
    avatarTrigger?.focus({ preventScroll: true });
  }

  /** Put the cropped picture in the draft; a previously picked copy it replaces is deleted. */
  function applyProfileImageCrop(crop: ProfileImageCrop): void {
    const editor = cropEditor;
    if (!editor) return;
    closeCropEditor();
    markPictureEdited();
    const replacedPending = pendingImagePath !== null && pendingImagePath !== editor.image.path
      ? pendingImagePath
      : null;
    draftImage = editor.image;
    draftImageCrop = { ...crop };
    if (editor.fromUpload) draftUseImage = true;
    pendingImagePath = editor.image.path === preferences.profileImagePath ? null : editor.image.path;
    if (replacedPending) void discardImageFile(replacedPending);
  }

  /** Close the crop editor; a newly picked copy that never entered the draft is deleted. */
  function cancelProfileImageCrop(): void {
    const editor = cropEditor;
    if (!editor) return;
    closeCropEditor();
    if (editor.ownsFile) void discardImageFile(editor.image.path);
  }

  function removeProfileImage(): void {
    if (!draftImagePath || savingImage) return;
    markPictureEdited();
    const pending = pendingImagePath;
    draftImage = null;
    draftImageCrop = { ...PROFILE_IMAGE_DEFAULT_CROP };
    pendingImagePath = null;
    if (pending) void discardImageFile(pending);
  }

  function handleIdentityKeydown(event: KeyboardEvent): void {
    if (event.key === "Enter") {
      event.preventDefault();
      if (canSave) void saveIdentity();
      return;
    }
    if (event.key === "Escape") {
      event.preventDefault();
      draftDisplayName = preferences.profileDisplayName;
      draftFullName = preferences.profileFullName;
      saveError = null;
      saved = false;
    }
  }
</script>

<div class="flex min-h-full flex-col">
  <section class="flex flex-1 flex-col gap-4 pb-6">
    <h2 class="px-1 text-[0.866667rem] font-semibold text-foreground">{t("settings.profileIdentity.heading")}</h2>

    <div class="flex flex-col gap-3">
      <div class="flex items-center justify-between gap-4 px-1 py-1 max-[480px]:flex-col max-[480px]:items-stretch max-[480px]:gap-2">
        <div class="min-w-0 flex-1">
          <label for="profile-display-name" class="text-[0.866667rem] text-foreground">
            {t("settings.profileIdentity.displayName")}
          </label>
          <div class="mt-0.5 text-[0.8rem] text-muted-foreground">
            {t("settings.profileIdentity.description")}
          </div>
        </div>

        <input
          id="profile-display-name"
          class="field w-44 min-w-0 px-2.5 text-left text-[0.8rem] font-medium text-foreground max-[480px]:w-full"
          value={draftDisplayName}
          maxlength={PROFILE_DISPLAY_NAME_MAX_CHARS}
          disabled={saving}
          onpointerdown={moveTextInputCaretToPointer}
          onkeydown={handleIdentityKeydown}
          oninput={(event) => {
            draftDisplayName = event.currentTarget.value;
            saveError = null;
            saved = false;
          }}
        />
      </div>
      <div class="flex items-center justify-between gap-4 px-1 py-1 max-[480px]:flex-col max-[480px]:items-stretch max-[480px]:gap-2">
        <div class="min-w-0 flex-1">
          <label for="profile-full-name" class="text-[0.866667rem] text-foreground">
            {t("settings.profileIdentity.fullName")}
          </label>
          <div class="mt-0.5 text-[0.8rem] text-muted-foreground">
            {t("settings.profileIdentity.fullNameDescription")}
          </div>
        </div>

        <input
          id="profile-full-name"
          class="field w-44 min-w-0 px-2.5 text-left text-[0.8rem] font-medium text-foreground max-[480px]:w-full"
          value={draftFullName}
          maxlength={PROFILE_FULL_NAME_MAX_CHARS}
          disabled={saving}
          onpointerdown={moveTextInputCaretToPointer}
          onkeydown={handleIdentityKeydown}
          oninput={(event) => {
            draftFullName = event.currentTarget.value;
            saveError = null;
            saved = false;
          }}
        />
      </div>
      <div class="flex items-center justify-between gap-4 px-1 py-1">
        <div class="min-w-0 flex-1">
          <div class="text-[0.866667rem] text-foreground">{t("settings.profileIdentity.picture")}</div>
          <div class="mt-0.5 text-[0.8rem] text-muted-foreground">
            {t("settings.profileIdentity.pictureDescription")}
          </div>
        </div>
        <button
          bind:this={avatarTrigger}
          type="button"
          class="group relative flex shrink-0 rounded-lg focus:outline-none focus-visible:ring-1 focus-visible:ring-ring"
          aria-label={t("settings.profileIdentity.editPicture")}
          aria-haspopup="dialog"
          aria-expanded={avatarPanelOpen}
          data-app-tooltip-disabled="true"
          onclick={() => (avatarPanelOpen = !avatarPanelOpen)}
        >
          <ProfileAvatar
            displayName={draftDisplayName || PROFILE_DISPLAY_NAME_FALLBACK}
            imagePath={draftUseImage ? draftImagePath : null}
            crop={draftImageCrop}
            color={draftColor}
            size={PROFILE_AVATAR_SIZE}
          />
          <span
            class={cn(
              "absolute -right-1 -bottom-1 flex size-5 items-center justify-center rounded-full border border-border bg-card text-foreground transition-colors group-hover:bg-accent group-active:bg-accent",
              avatarPanelOpen && "bg-accent",
            )}
            aria-hidden="true"
          >
            {#if savingImage}
              <LoaderCircle size={11} strokeWidth={2} class="animate-spin motion-reduce:animate-none" />
            {:else}
              <Pencil size={10} strokeWidth={2} />
            {/if}
          </span>
        </button>
      </div>
      {#if avatarPanelOpen && avatarTrigger}
        <ProfileAvatarPanel
          anchor={avatarTrigger}
          hasImage={draftImagePath !== null}
          useImage={draftUseImage}
          color={draftColor}
          busy={savingImage || saving}
          onUpload={() => void chooseProfileImage()}
          onAdjust={adjustProfileImage}
          onRemove={removeProfileImage}
          onUseImageChange={(useImage) => {
            draftUseImage = useImage;
            markPictureEdited();
          }}
          onColorChange={(color) => {
            draftColor = color;
            markPictureEdited();
          }}
          onClose={() => (avatarPanelOpen = false)}
        />
      {/if}
      {#if cropEditor}
        {#key cropEditor.image.path}
          <ProfileImageCropDialog
            imagePath={cropEditor.image.path}
            initialCrop={cropEditor.crop}
            onApply={applyProfileImageCrop}
            onCancel={cancelProfileImageCrop}
          />
        {/key}
      {/if}
    </div>
  </section>

  <footer class="sticky bottom-0 shrink-0 pt-3">
    <div class="flex flex-wrap items-center justify-between gap-2 border-t border-border/70 pt-3">
      <div class="min-w-0 flex-1 px-1 text-[0.8rem] leading-5">
        {#if saveError}
          <Notice message={t("settings.profileIdentity.saveFailed", saveError)} />
        {:else if validationMessage}
          <span role="alert" class="text-destructive">
            {validationMessage}
          </span>
        {/if}
      </div>
      <button
        type="button"
        onclick={() => void saveIdentity()}
        disabled={!hasSaveableChange || saving}
        class={cn(
          "flex h-8 shrink-0 items-center justify-center gap-1.5 rounded-md bg-primary px-3 text-[0.8rem] font-medium text-primary-foreground transition-colors disabled:pointer-events-none",
          hasSaveableChange || saving ? "hover:bg-primary/90" : "opacity-55",
        )}
      >
        <Save size={13} strokeWidth={2.25} class="shrink-0" />
        <span>{t("settings.profileIdentity.save")}</span>
      </button>
    </div>
  </footer>
</div>
