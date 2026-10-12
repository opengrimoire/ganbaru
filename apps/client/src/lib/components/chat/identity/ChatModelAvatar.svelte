<script lang="ts">
  import ChatProviderIcon from "./ChatProviderIcon.svelte";

  let {
    familyId,
    label,
    size = 36,
  }: {
    familyId: string;
    label: string;
    size?: number;
  } = $props();
</script>

<span
  class="model-avatar"
  class:claude={familyId === "claude"}
  style:width={`${size}px`}
  style:height={`${size}px`}
  role="img"
  aria-label={label}
>
  <ChatProviderIcon
    familyId={familyId}
    {label}
    monochromeColor="#000000"
    colorOverride={familyId === "claude" ? "#FAFAF8" : null}
    size={Math.round(size * 0.58)}
  />
</span>

<style>
  .model-avatar {
    display: inline-grid;
    flex: 0 0 auto;
    place-items: center;
    overflow: hidden;
    border-radius: var(--chat-participant-avatar-radius, 22%);
    background: #f2f2f2;
  }

  .model-avatar.claude { background: #D97656; }
</style>
