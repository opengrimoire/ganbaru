<script lang="ts">
  import ConnectSteps, { type ConnectSource } from "./ConnectSteps.svelte";

  let { source, autoScan, onSubmit }: {
    source: ConnectSource;
    autoScan: boolean;
    onSubmit: (code: string) => void;
  } = $props();
</script>

<ConnectSteps
  platform="android"
  {source}
  {autoScan}
  busy={false}
  enrollmentCode={null}
  error={null}
  panelId="panel"
  labelledBy="tab"
  {onSubmit}
  onEdit={() => undefined}
  onServerActivated={() => undefined}
/>
