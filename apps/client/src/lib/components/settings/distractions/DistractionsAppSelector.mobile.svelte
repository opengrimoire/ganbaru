<script lang="ts" module>
  export interface DistractionsAppSelection {
    name: string;
    matchNames: readonly string[];
  }

  export type DistractionsAppSelectorMode = "multi" | "single";
</script>

<script lang="ts">
  let {
    title: _title,
    mode: _mode = "multi",
    existingNames: _existingNames,
    protectAppSelf: _protectAppSelf = false,
    onAdd: _onAdd,
    onRemove: _onRemove,
    onSelect: _onSelect,
    onCancel: _onCancel,
  }: {
    title: string;
    mode?: DistractionsAppSelectorMode;
    existingNames: readonly string[];
    protectAppSelf?: boolean;
    onAdd?: (app: DistractionsAppSelection) => boolean;
    onRemove?: (name: string) => void;
    onSelect?: (app: DistractionsAppSelection) => void;
    onCancel: () => void;
  } = $props();
</script>
