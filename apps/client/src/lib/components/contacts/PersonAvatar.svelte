<script lang="ts">
  import ProfileAvatar from "$lib/components/profile/ProfileAvatar.svelte";
  import type { EventColor } from "$lib/calendar/types";

  /** Avatar of another person: their initials on the palette color their contact card carries. Pictures are later work. */
  let {
    displayName,
    color,
    size = 26,
  }: {
    displayName: string;
    color: EventColor;
    size?: number;
  } = $props();
</script>

<span class="person-avatar" style={`width:${size}px;height:${size}px`} aria-hidden="true">
  <ProfileAvatar {displayName} {color} {size} />
</span>

<style>
  .person-avatar { display: inline-grid; flex: 0 0 auto; overflow: hidden; border-radius: 22%; }
  .person-avatar :global(.profile-avatar) { display: grid; }
</style>
