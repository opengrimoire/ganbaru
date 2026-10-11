<script lang="ts" module>
  /** Avatar size in comments, shared by authors, composers, and the thread line that starts below an avatar. */
  export const NOTES_COMMENT_AVATAR_SIZE = 18;
</script>

<script lang="ts">
  import LocalPersonAvatar from "$lib/components/contacts/LocalPersonAvatar.svelte";
  import ProfileAvatar from "$lib/components/profile/ProfileAvatar.svelte";

  /**
   * A round comment avatar: the local profile picture for own comments and composers, otherwise initials from the
   * author's name.
   */
  let {
    own,
    name = "",
    size = NOTES_COMMENT_AVATAR_SIZE,
  }: {
    own: boolean;
    /** Author name for the initials; own avatars read the local profile instead. */
    name?: string;
    size?: number;
  } = $props();
</script>

<span class="comment-avatar" style={`width:${size}px;height:${size}px`} aria-hidden="true">
  {#if own}
    <LocalPersonAvatar {size} />
  {:else}
    <ProfileAvatar displayName={name} {size} />
  {/if}
</span>

<style>
  .comment-avatar {
    --chat-participant-avatar-radius: 50%;
    display: inline-grid;
    flex: 0 0 auto;
    overflow: hidden;
    border-radius: 50%;
  }
  .comment-avatar :global(.profile-avatar) {
    display: grid;
  }
</style>
