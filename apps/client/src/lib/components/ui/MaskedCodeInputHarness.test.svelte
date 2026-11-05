<script lang="ts">
  import MaskedCodeInput from "./MaskedCodeInput.svelte";

  let { onInput }: { onInput: () => void } = $props();

  let value = $state("");

  /** The field's whole code, as its owner would submit it. */
  export function current(): string {
    return value;
  }

  /** Replace the code from outside, as a dialog does when it forgets a refused kit. */
  export function set(next: string): void {
    value = next;
  }
</script>

<MaskedCodeInput bind:value id="code" clearLabel="Clear code" oninput={onInput} />
