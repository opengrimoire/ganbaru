<script lang="ts">
  let { onOpenSoundscapes }: { onOpenSoundscapes: () => void } = $props();

  /** Matches the desktop control; platforms without soundscapes have nothing to open. */
  export function toggleOpen(): void {}

  export function isOpen(): boolean {
    return false;
  }

  export function togglePlayback(): void {}
</script>
