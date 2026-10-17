<script lang="ts">
  const props: {
    projectId: string;
    saving?: boolean;
    onDirtyChange?: (dirty: boolean) => void;
    onBusyChange?: (busy: boolean) => void;
  } = $props();

  /** Mobile has no working-folder settings to persist. */
  export async function saveDraft(): Promise<void> {}

  /** Mobile has no working-folder draft to discard. */
  export function discardDraft(): void {}

  $effect(() => {
    Reflect.ownKeys(props);
  });
</script>
