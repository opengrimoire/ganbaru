<script lang="ts">
  import Switch from "./Switch.svelte";

  let {
    label,
    description,
    checked,
    onChange,
    disabled = false,
  }: {
    label: string;
    description?: string;
    checked: boolean;
    onChange: (checked: boolean) => void;
    disabled?: boolean;
  } = $props();
</script>

<div class="flex items-center justify-between gap-4 px-1 py-1 max-[480px]:items-start">
  <div class="min-w-0 flex-1">
    <div class="text-[0.866667rem] text-foreground">{label}</div>
    {#if description}
      <div class="mt-0.5 text-[0.8rem] text-muted-foreground">{description}</div>
    {/if}
  </div>
  <Switch {checked} {onChange} ariaLabel={label} {disabled} />
</div>
