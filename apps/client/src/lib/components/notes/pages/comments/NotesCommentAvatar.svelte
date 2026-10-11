<script lang="ts">
  import LocalPersonAvatar from "$lib/components/contacts/LocalPersonAvatar.svelte";
  import ProfileAvatar from "$lib/components/profile/ProfileAvatar.svelte";

  /** A comment author's avatar: the local profile picture for own comments, otherwise initials from the name. */
  let {
    own,
    name,
    size,
  }: {
    own: boolean;
    name: string;
    size: number;
  } = $props();
</script>

{#if own}
  <LocalPersonAvatar {size} />
{:else}
  <span class="comment-avatar" style={`width:${size}px;height:${size}px`} aria-hidden="true">
    <ProfileAvatar displayName={name} {size} />
  </span>
{/if}

<style>
  .comment-avatar {
    display: inline-grid;
    flex: 0 0 auto;
    overflow: hidden;
    border-radius: 22%;
  }
  .comment-avatar :global(.profile-avatar) {
    display: grid;
  }
</style>
