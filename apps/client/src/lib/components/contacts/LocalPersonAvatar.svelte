<script lang="ts">
  import ProfileAvatar from "$lib/components/profile/ProfileAvatar.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getPreferences } from "$lib/stores/preferences.svelte";

  /** Avatar of the person using this device, read from the folder-local profile. */
  let { size = 26 }: { size?: number } = $props();

  const preferences = getPreferences();
  const { t } = getLocalization();
  const displayName = $derived(preferences.profileDisplayName.trim() || t("contacts.you"));
</script>

<span class="local-person-avatar" style={`width:${size}px;height:${size}px`} aria-hidden="true">
  <ProfileAvatar {displayName} imagePath={preferences.profileAvatarImagePath} crop={preferences.profileImageCrop} color={preferences.profileColor} {size} />
</span>

<style>
  .local-person-avatar { display: inline-grid; flex: 0 0 auto; overflow: hidden; border-radius: 22%; }
  .local-person-avatar :global(.profile-avatar) { display: grid; }
</style>
