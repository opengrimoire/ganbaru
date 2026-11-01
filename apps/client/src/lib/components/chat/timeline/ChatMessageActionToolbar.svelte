<script lang="ts">
  import Check from "@lucide/svelte/icons/check";
  import Copy from "@lucide/svelte/icons/copy";
  import EllipsisVertical from "@lucide/svelte/icons/ellipsis-vertical";
  import SmilePlus from "@lucide/svelte/icons/face-slightly-smiling-plus";
  import { onDestroy } from "svelte";
  import { loadChatMessageProvenance } from "$lib/api/chat/provenance";
  import type { SyncDeviceRef } from "$lib/api/sync";
  import type IconPicker from "$lib/components/icon-picker/IconPicker.svelte";
  import type { ChatMessageActionTarget } from "$lib/chat/timeline/message-action-target";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { useChat } from "$lib/stores/chat.svelte";
  import { getPreferences } from "$lib/stores/preferences.svelte";
  import { syncDeviceName } from "$lib/sync/status";
  import { anchoredPopover } from "$lib/utils/anchored-popover";
  import { writeTextToClipboard } from "$lib/utils/clipboard";
  import { portal } from "$lib/utils/portal";

  let {
    target,
    visible = false,
    placement = "participant-header",
    onError,
  }: {
    target: ChatMessageActionTarget;
    visible?: boolean;
    placement?: "participant-header" | "grouped-message";
    onError?: (error: unknown) => void;
  } = $props();

  const { t } = getLocalization();
  const chat = useChat();
  const preferences = getPreferences();
  let copied = $state(false);
  let copiedResetTimer: ReturnType<typeof setTimeout> | null = null;
  let ReactionPicker = $state<typeof IconPicker | null>(null);
  let reactionPickerValue = $state("emoji:👍");
  let moreButton = $state<HTMLButtonElement>();
  let sentFromDevices = $state<readonly SyncDeviceRef[] | null>(null);
  let provenanceRequest = 0;
  const localDisplayName = $derived(preferences.profileDisplayName.trim() || t("chat.timeline.you"));
  const hasProvenance = $derived(target.kind === "organizational");

  function reportError(error: unknown): void {
    if (onError) {
      onError(error);
      return;
    }
    console.error("Could not complete Chat message action", error);
  }

  async function copyMessage(): Promise<void> {
    try {
      await writeTextToClipboard(target.copyText);
      copied = true;
      if (copiedResetTimer) clearTimeout(copiedResetTimer);
      copiedResetTimer = setTimeout(() => {
        copied = false;
        copiedResetTimer = null;
      }, 2_000);
    } catch (error: unknown) {
      reportError(error);
    }
  }

  async function openReactionPicker(): Promise<void> {
    if (ReactionPicker) return;
    try {
      ReactionPicker = (await import("$lib/components/icon-picker/IconPicker.svelte")).default;
    } catch (error: unknown) {
      reportError(error);
    }
  }

  function toggleReaction(value: string): void {
    reactionPickerValue = value;
    chat.toggleSessionMessageReaction(target.reactionKey, value, localDisplayName);
  }

  function closeSentFrom(): void {
    provenanceRequest += 1;
    sentFromDevices = null;
  }

  /** Opens the devices that sealed the message; a message without a sync record opens nothing. */
  async function toggleSentFrom(): Promise<void> {
    if (sentFromDevices) {
      closeSentFrom();
      return;
    }
    provenanceRequest += 1;
    const request = provenanceRequest;
    try {
      const { devices } = await loadChatMessageProvenance(target.sourceId);
      if (request === provenanceRequest && devices.length > 0) sentFromDevices = devices;
    } catch (error: unknown) {
      reportError(error);
    }
  }

  function handleWindowKeydown(event: KeyboardEvent): void {
    if (event.key === "Escape" && sentFromDevices) closeSentFrom();
  }

  onDestroy(() => {
    if (copiedResetTimer) clearTimeout(copiedResetTimer);
  });
</script>

<svelte:window onkeydown={handleWindowKeydown} />

<div
  class="message-action-controls surface-floating"
  class:visible
  class:grouped-message={placement === "grouped-message"}
  data-message-action-kind={target.kind}
  data-message-action-source-id={target.sourceId}
>
  {#if ReactionPicker}
    <ReactionPicker
      value={reactionPickerValue}
      onChange={toggleReaction}
      ariaLabel={t("chat.organization.addReaction")}
      showIcons={false}
      showUpload={false}
      showRemove={false}
      initiallyOpen
    >
      {#snippet trigger({ open, toggle, panelId })}
        <button
          type="button"
          class="message-action-button"
          class:active={open}
          aria-label={t("chat.organization.addReaction")}
          aria-haspopup="dialog"
          aria-expanded={open}
          aria-controls={panelId}
          data-app-tooltip={t("chat.organization.addReaction")}
          onclick={toggle}
        ><SmilePlus size={14} /></button>
      {/snippet}
    </ReactionPicker>
  {:else}
    <button
      type="button"
      class="message-action-button"
      aria-label={t("chat.organization.addReaction")}
      data-app-tooltip={t("chat.organization.addReaction")}
      onclick={() => void openReactionPicker()}
    ><SmilePlus size={14} /></button>
  {/if}
  <button
    type="button"
    class="message-action-button"
    class:copied
    aria-label={copied ? t("chat.timeline.copied") : t("chat.timeline.copy")}
    title={copied ? t("chat.timeline.copied") : t("chat.timeline.copy")}
    data-app-tooltip-keep-on-click="true"
    onclick={() => void copyMessage()}
  >{#if copied}<Check size={13} />{:else}<Copy size={13} />{/if}</button>
  <button
    bind:this={moreButton}
    type="button"
    class="message-action-button"
    class:active={sentFromDevices !== null}
    aria-label={t("chat.moreActions")}
    aria-haspopup={hasProvenance ? "dialog" : undefined}
    aria-expanded={hasProvenance ? sentFromDevices !== null : undefined}
    data-app-tooltip={t("chat.moreActions")}
    onclick={hasProvenance ? () => void toggleSentFrom() : undefined}
  ><EllipsisVertical size={14} /></button>
</div>

{#if sentFromDevices}
  <div
    use:portal
    use:anchoredPopover={{ anchor: moreButton, onOutside: closeSentFrom, horizontalAlign: "end" }}
    class="surface-floating surface-floating-body fixed z-90 w-floating-sm"
    role="dialog"
    aria-label={t("chat.organization.sentFrom")}
    data-app-floating-surface
  >
    <p class="menu-label">{t("chat.organization.sentFrom")}</p>
    <ul>
      {#each sentFromDevices as device (device.deviceId)}
        <li class="sent-from-device text-panel">{syncDeviceName(device, t("chat.conflict.thisDevice"), t("chat.conflict.otherDevice"))}</li>
      {/each}
    </ul>
  </div>
{/if}

<style>
  .message-action-controls { pointer-events:none; position:absolute; top:var(--chat-message-action-anchor-bottom,1.25rem); right:0; z-index:2; display:flex; align-items:center; overflow:hidden; opacity:0; transform:translateY(-100%); }
  .message-action-controls.grouped-message { top:0; transform:translateY(-50%); }
  .message-action-controls.visible,.message-action-controls:focus-within { pointer-events:auto; opacity:1; }
  .message-action-button { display:inline-grid; width:1.75rem; height:1.75rem; flex:0 0 auto; place-items:center; border-radius:0; color:var(--muted-foreground); }
  .message-action-button:focus-visible,.message-action-button.active,.message-action-button.copied { background:var(--accent); color:var(--foreground); }
  @media (hover:hover) { .message-action-button:hover { background:var(--accent); color:var(--foreground); } }
  .sent-from-device { padding:0.25rem 0.5rem; overflow-wrap:anywhere; }
</style>
