<script lang="ts">
  import type { ChatTeammateChannelAccessInput } from "$lib/chat/contracts";
  import {
    channelCapabilityPreset,
    type ChatChannelCapabilityPreset,
  } from "$lib/chat/teammates/access";
  import ChatControlMenu, { type ChatControlOption } from "$lib/components/chat/composer/ChatControlMenu.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";

  type SelectablePreset = Exclude<ChatChannelCapabilityPreset, "custom">;

  let {
    channels,
    disabled = false,
    onPresetChange,
  }: {
    channels: readonly ChatTeammateChannelAccessInput[];
    disabled?: boolean;
    onPresetChange: (preset: SelectablePreset) => void;
  } = $props();

  const { t } = getLocalization();
  const MIXED_VALUE = "__mixed__";
  const CUSTOM_VALUE = "__custom__";
  const presets = $derived(channels.map((channel) => channelCapabilityPreset(channel.capabilities)));
  const uniformPreset = $derived(presets.length > 0 && presets.every((preset) => preset === presets[0])
    ? presets[0]
    : null);
  const presetValue = $derived(uniformPreset === "custom"
    ? CUSTOM_VALUE
    : uniformPreset ?? MIXED_VALUE);
  const presetPlaceholder = $derived<ChatControlOption>({
    value: presetValue,
    label: uniformPreset === "custom"
      ? t("settings.chat.teammates.presets.custom")
      : t("settings.chat.teammates.mixedChannelBehavior"),
    icon: "sliders",
  });
  const presetOptions = $derived<ChatControlOption[]>([
    {
      value: "isolatedResponder",
      label: t("settings.chat.teammates.presetControls.isolatedResponder.label"),
      description: t("settings.chat.teammates.presetControls.isolatedResponder.description"),
      icon: "bot",
    },
    {
      value: "collaborator",
      label: t("settings.chat.teammates.presetControls.collaborator.label"),
      description: t("settings.chat.teammates.presetControls.collaborator.description"),
      icon: "messages-square",
    },
    {
      value: "contextSource",
      label: t("settings.chat.teammates.presetControls.contextSource.label"),
      description: t("settings.chat.teammates.presetControls.contextSource.description"),
      icon: "book-open",
    },
  ]);
</script>

<div class="scope-controls">
  <div class="scope-control">
    <ChatControlMenu
      value={presetValue}
      options={presetOptions}
      placeholder={presetPlaceholder}
      ariaLabel={t("settings.chat.teammates.channelBehavior")}
      onChange={(value) => onPresetChange(value as SelectablePreset)}
      {disabled}
      showTooltip={false}
    />
  </div>
</div>

<style>
  .scope-controls { display:flex; min-width:0; align-items:center; justify-content:flex-end; }
  .scope-control { min-width:0; }
  .scope-control :global(.control-trigger) { height:1.9rem; max-width:11.5rem; border:1px solid var(--border); border-radius:0.45rem; background:var(--background); padding-inline:0.5rem; color:var(--foreground); }
  .scope-control :global(.control-trigger:hover),.scope-control :global(.control-trigger[aria-expanded="true"]) { background:var(--accent); }
  @media (max-width:700px) { .scope-controls { justify-content:flex-start; } }
</style>
