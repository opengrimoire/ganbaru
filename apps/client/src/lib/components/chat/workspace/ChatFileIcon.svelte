<script lang="ts">
  import { chatFilePresentation } from "$lib/chat/workspace/file-icon-theme";
  import { chatFileIconUrl } from "$lib/chat/workspace/file-icons.generated";

  let { path, size = 14 }: { path: string; size?: number } = $props();
  const presentation = $derived(chatFilePresentation(path));
</script>

<span class="file-icon" style={`width:${size}px;height:${size}px;`} aria-hidden="true">
  <img
    class="file-icon-image"
    class:light-variant={presentation.darkIcon !== undefined}
    src={chatFileIconUrl(presentation.icon)}
    alt=""
    draggable="false"
  />
  {#if presentation.darkIcon}
    <img
      class="file-icon-image dark-variant"
      src={chatFileIconUrl(presentation.darkIcon)}
      alt=""
      draggable="false"
    />
  {/if}
</span>

<style>
  .file-icon { position: relative; display: inline-block; flex: 0 0 auto; color: var(--muted-foreground); }
  .file-icon-image { display: block; width: 100%; height: 100%; object-fit: contain; }
  .dark-variant { display: none; }
  :global(.dark) .light-variant { display: none; }
  :global(.dark) .dark-variant { display: block; }
</style>
