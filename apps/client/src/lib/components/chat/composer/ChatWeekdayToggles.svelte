<script lang="ts">
  import { CHAT_ALL_WEEKDAYS_MASK } from "$lib/chat/contracts";
  import { chatWeekdayBit } from "$lib/chat/composer/message-scheduling";
  import { chatWeekdayNames } from "$lib/chat/composer/recurrence-presentation";
  import { getLocalization } from "$lib/i18n/translator.svelte";

  /**
   * Seven day toggles over a weekday mask (bit 0 Monday through bit 6 Sunday). The last
   * selected day cannot be turned off, so the mask never becomes empty.
   */
  let {
    mask,
    label,
    disabled = false,
    onChange,
  }: {
    mask: number;
    label: string;
    disabled?: boolean;
    onChange: (mask: number) => void;
  } = $props();

  const localization = getLocalization();
  const names = $derived(chatWeekdayNames(CHAT_ALL_WEEKDAYS_MASK, localization.locale));

  function toggle(isoWeekday: number): void {
    const next = mask ^ chatWeekdayBit(isoWeekday);
    if (next !== 0) onChange(next);
  }
</script>

<div class="weekday-toggles" role="group" aria-label={label}>
  {#each names as name, index (index)}
    {@const isoWeekday = index + 1}
    {@const selected = (mask & chatWeekdayBit(isoWeekday)) !== 0}
    <button type="button" class="weekday" aria-pressed={selected} {disabled} onclick={() => toggle(isoWeekday)}>{name}</button>
  {/each}
</div>

<style>
  .weekday-toggles { display:grid; grid-template-columns:repeat(7,minmax(0,1fr)); gap:0.15rem; }
  .weekday { min-width:0; overflow:hidden; border-radius:var(--floating-item-radius); padding:0.2rem 0; color:var(--muted-foreground); font-size:var(--panel-detail-font-size); text-overflow:ellipsis; white-space:nowrap; }
  .weekday:hover:not(:disabled) { background:var(--accent); color:var(--foreground); }
  .weekday[aria-pressed="true"] { background:var(--primary); color:var(--primary-foreground); }
  .weekday:disabled { opacity:0.45; }
</style>
