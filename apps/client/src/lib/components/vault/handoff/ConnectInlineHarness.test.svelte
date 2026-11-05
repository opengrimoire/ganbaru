<script lang="ts">
  import ConnectSourceTabs from "./ConnectSourceTabs.svelte";
  import ConnectSteps, { type ConnectSource } from "./ConnectSteps.svelte";

  let { onSubmit }: { onSubmit: (code: string) => void } = $props();

  const sources: { id: "phone" | ConnectSource; label: string }[] = [
    { id: "phone", label: "your phone" },
    { id: "computer", label: "another computer" },
    { id: "server", label: "a server" },
  ];
  let source = $state<"phone" | ConnectSource>("phone");
</script>

<h1>
  Connect to
  <ConnectSourceTabs {sources} active={source} idPrefix="tab" panelId="panel" onChoose={(next) => { source = next; }} />
</h1>
<ConnectSteps
  source={source === "phone" ? null : source}
  busy={false}
  enrollmentCode={null}
  error={null}
  panelId="panel"
  labelledBy={`tab-${source}`}
  {onSubmit}
  onEdit={() => undefined}
  onServerActivated={() => undefined}
>
  <p data-phone-panel>QR code</p>
</ConnectSteps>
