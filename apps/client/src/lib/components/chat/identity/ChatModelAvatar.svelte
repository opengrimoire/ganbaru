<script lang="ts">
  import ChatIconTile from "./ChatIconTile.svelte";
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

  const CLAUDE_TILE_BACKGROUND = "#D97656";
  const CLAUDE_LOGO_COLOR = "#FAFAF8";
  const isClaude = $derived(familyId === "claude");
</script>

<ChatIconTile class="model-avatar" {size} {label} background={isClaude ? CLAUDE_TILE_BACKGROUND : undefined}>
  <ChatProviderIcon
    familyId={familyId}
    {label}
    monochromeColor="#000000"
    colorOverride={isClaude ? CLAUDE_LOGO_COLOR : null}
    size={Math.round(size * 0.58)}
  />
</ChatIconTile>
