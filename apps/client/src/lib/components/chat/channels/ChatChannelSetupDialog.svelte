<script lang="ts">
  import { onMount, tick, untrack } from "svelte";
  import Hash from "@lucide/svelte/icons/hash";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import Settings2 from "@lucide/svelte/icons/settings-2";
  import UserRoundPlus from "@lucide/svelte/icons/user-round-plus";
  import X from "@lucide/svelte/icons/x";
  import * as chatApi from "$lib/api/chat";
  import type { ChatChannelRead, ChatChannelMembershipRemovalPreview } from "$lib/chat/contracts";
  import type { ChatSidebarSection } from "$lib/chat/channel-sections";
  import { summarizeChannelMembers } from "$lib/chat/agents/channel-members";
  import ChatParticipantAvatar from "$lib/components/chat/identity/ChatParticipantAvatar.svelte";
  import LocalPersonAvatar from "$lib/components/contacts/LocalPersonAvatar.svelte";
  import ParticipantPicker from "$lib/components/contacts/ParticipantPicker.svelte";
  import MemberRow from "$lib/components/contacts/MemberRow.svelte";
  import MemberRoleChip from "$lib/components/contacts/MemberRoleChip.svelte";
  import MemberRoleSelect from "$lib/components/contacts/MemberRoleSelect.svelte";
  import ConfirmDialog from "$lib/components/ui/ConfirmDialog.svelte";
  import Select from "$lib/components/ui/Select.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { chatErrorMessage } from "$lib/chat/error-presentation";
  import { activateModalFocus, activateModalKeyboardLayer, trapModalTabKey } from "$lib/modal-focus";
  import { useChatPaneEvents } from "$lib/chat/pane-events";
  import { useChat } from "$lib/stores/chat.svelte";
  import { getPreferences } from "$lib/stores/preferences.svelte";
  import { useProjects } from "$lib/stores/projects.svelte";
  import { getSettingsLauncher } from "$lib/stores/settings-launcher.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { CHAT_OPEN_MEMBERS_EVENT, type ChatOpenMembersDetail } from "./members-panel-events";

  let {
    channel = null,
    sections,
    initialSectionId = null,
    onSaved,
    onCancel,
  }: {
    channel?: ChatChannelRead | null;
    sections: ChatSidebarSection[];
    initialSectionId?: string | null;
    onSaved: (channel: ChatChannelRead, sectionId: string | null) => void;
    onCancel: () => void;
  } = $props();

  const MEMBER_AVATAR_SIZE = 26;

  const chat = useChat();
  const paneEvents = useChatPaneEvents();
  const preferences = getPreferences();
  const projects = useProjects();
  const settingsLauncher = getSettingsLauncher();
  const { t } = getLocalization();
  const currentChannel = untrack(() => channel);
  const projectId = $derived(currentChannel?.projectId ?? projects.selectedProjectId ?? "");
  const sectionOptions = $derived([
    { value: "", label: t("chat.channels.defaultSection") },
    ...sections.map((section) => ({ value: section.id, label: section.name })),
  ]);
  const currentMembers = summarizeChannelMembers(currentChannel?.memberships ?? []);
  const currentAgentIds = currentMembers.agentIds;

  let dialog = $state<HTMLDivElement | null>(null);
  let nameInput = $state<HTMLInputElement | null>(null);
  let addMembersButton = $state<HTMLButtonElement | null>(null);
  let name = $state(currentChannel?.name ?? "");
  let topic = $state(currentChannel?.topic ?? "");
  let sectionId = $state(untrack(() => initialSectionId ?? ""));
  let selectedAgentIds = $state<ReadonlySet<string>>(new Set(currentAgentIds));
  let pickerOpen = $state(false);
  let saving = $state(false);
  let error = $state<string | null>(null);
  let removal = $state<{ agentId: string; preview: ChatChannelMembershipRemovalPreview } | null>(null);
  let removalBusy = $state(false);

  const memberAgents = $derived(chat.agents.filter((agent) => selectedAgentIds.has(agent.participant.id)));
  const localName = $derived(preferences.profileDisplayName.trim() || t("contacts.you"));
  const localDetail = $derived(preferences.profileDisplayName.trim() ? t("contacts.you") : null);
  const removalParticipant = $derived.by(() => {
    const pending = removal;
    if (!pending) return null;
    return chat.agents.find((agent) => agent.participant.id === pending.agentId)?.participant ?? null;
  });

  function addMembers(agentIds: string[]): void {
    selectedAgentIds = new Set([...selectedAgentIds, ...agentIds]);
  }

  /**
   * Agents come from installed providers, so a new one starts in Settings > Agents. Settings covers the dialog,
   * so it closes now and comes back for the same channel once Settings is gone.
   */
  function setUpProvider(): void {
    const channelId = currentChannel?.id;
    onCancel();
    settingsLauncher.open("agents", {
      agentsSubsection: "providers",
      onClosed: () => {
        if (channelId) paneEvents.dispatchEvent(new CustomEvent("ganbaru:chat-edit-channel", { detail: { channelId } }));
        else paneEvents.dispatchEvent(new Event("ganbaru:chat-new-channel"));
      },
    });
  }

  function dropMember(agentId: string): void {
    const next = new Set(selectedAgentIds);
    next.delete(agentId);
    selectedAgentIds = next;
  }

  async function removeMember(agentId: string): Promise<void> {
    if (!currentChannel || !currentAgentIds.has(agentId)) {
      dropMember(agentId);
      return;
    }
    if (removalBusy) return;
    removalBusy = true;
    error = null;
    try {
      const preview = await chatApi.previewChatChannelMembershipRemoval({ agentId, channelId: currentChannel.id });
      if (preview.willRevokeActiveWork) removal = { agentId, preview };
      else dropMember(agentId);
    } catch (cause: unknown) {
      error = chatErrorMessage(cause, t("chatErrors.actions.membersAction"));
    } finally {
      removalBusy = false;
    }
  }

  /** Agent access, instructions, and models live in the channel's details, so this opens them for the channel. */
  function openAgentSettings(channelId: string): void {
    paneEvents.dispatchEvent(new CustomEvent(CHAT_OPEN_MEMBERS_EVENT, {
      detail: { channelId, addMembers: false } satisfies ChatOpenMembersDetail,
    }));
  }

  function configureMember(): void {
    const channelId = currentChannel?.id;
    onCancel();
    if (channelId) openAgentSettings(channelId);
  }

  /** Adds and removes the agents this dialog changed, and reports whether any were added. */
  async function applyAgentChanges(channelId: string): Promise<boolean> {
    const added = [...selectedAgentIds].filter((agentId) => !currentAgentIds.has(agentId));
    const removed = [...currentAgentIds].filter((agentId) => !selectedAgentIds.has(agentId));
    for (const agentId of removed) await chatApi.removeChatChannelAgent({ channelId, agentId });
    for (const agentId of added) await chatApi.addChatChannelAgent({ channelId, agentId });
    if (added.length > 0 || removed.length > 0) await chat.channelAgentsChanged(channelId);
    return added.length > 0;
  }

  async function save(): Promise<void> {
    if (!projectId || saving || !name.trim()) return;
    saving = true;
    error = null;
    try {
      let saved = currentChannel
        ? await chat.updateChannelDetails(currentChannel, name, topic)
        : await chat.createChannel({ id: `channel:${crypto.randomUUID()}`, projectId, name, topic });
      const addedAgents = await applyAgentChanges(saved.id);
      saved = chat.activeChannels.find((entry) => entry.id === saved.id) ?? saved;
      onSaved(saved, sectionId || null);
      if (addedAgents) openAgentSettings(saved.id);
    } catch (cause: unknown) {
      error = chatErrorMessage(cause, t("chatErrors.actions.channelAction"));
    } finally {
      saving = false;
    }
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (removal) return;
    if (event.key === "Tab" && dialog) {
      trapModalTabKey(dialog, event);
      return;
    }
    if (event.key !== "Escape" || dialog?.querySelector("[data-app-floating-surface]")) return;
    event.preventDefault();
    onCancel();
  }

  onMount(() => {
    const deactivateKeyboard = activateModalKeyboardLayer(handleKeydown);
    let deactivateFocus = (): void => undefined;
    void tick().then(() => {
      if (dialog) deactivateFocus = activateModalFocus(dialog, nameInput?.disabled ? null : nameInput);
    });
    return () => {
      deactivateKeyboard();
      deactivateFocus();
    };
  });
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="fixed inset-0 z-90 flex items-center justify-center p-4" onclick={onCancel}>
  <div class="surface-backdrop absolute inset-0"></div>
  <div
    bind:this={dialog}
    class="channel-dialog surface-dialog relative z-10 flex max-h-full w-full max-w-md flex-col outline-none"
    style="--foreground: var(--card-foreground);"
    role="dialog"
    aria-modal="true"
    aria-labelledby="channel-dialog-title"
    tabindex="-1"
    data-floating-root
    onclick={(event) => event.stopPropagation()}
  >
  <form class="contents" onsubmit={(event) => { event.preventDefault(); void save(); }}>
    <header class="flex items-center justify-between gap-3 px-5 pt-4 pb-1">
      <h2 id="channel-dialog-title" class="truncate text-[1rem] font-semibold">{currentChannel ? `#${currentChannel.name}` : t("chat.channels.createTitle")}</h2>
      <button type="button" class="dialog-icon-button" aria-label={t("common.close")} onclick={onCancel}><X size={16} /></button>
    </header>

    <div class="dialog-body flex min-h-0 flex-col gap-4 overflow-y-auto px-5 py-3 text-panel">
      <label class="grid gap-1.5">
        <span class="field-label">{t("chat.channels.name")}</span>
        <div class="field flex items-center gap-1 pl-2.5"><Hash size={14} class="shrink-0 text-muted-foreground" /><input bind:this={nameInput} class="field-bare min-w-0 flex-1" bind:value={name} maxlength="80" required disabled={currentChannel?.isDefault} /></div>
      </label>
      <label class="grid gap-1.5">
        <span class="field-label">{t("chat.channels.topic")}</span>
        <input class="field" bind:value={topic} maxlength="250" placeholder={t("chat.channels.topicPlaceholder")} />
      </label>
      {#if sections.length > 0}
        <div class="grid gap-1.5">
          <span class="field-label">{t("chat.channels.section")}</span>
          <Select inline class="w-full" value={sectionId} options={sectionOptions} ariaLabel={t("chat.channels.section")} onChange={(value) => { sectionId = value; }} />
        </div>
      {/if}

      <div class="grid gap-1.5">
        <div class="flex items-center justify-between gap-2">
          <span class="field-label">{t("chat.organization.members")}</span>
          <button bind:this={addMembersButton} type="button" class="member-add-button" aria-haspopup="dialog" aria-expanded={pickerOpen} onclick={() => { pickerOpen = true; }}><UserRoundPlus size={13} />{t("chat.organization.addMembers")}</button>
        </div>
        <span class="member-group-label">{t("chat.organization.people")}</span>
        <ul class="member-list">
          <MemberRow name={localName} detail={localDetail}>
            {#snippet avatar()}<LocalPersonAvatar size={MEMBER_AVATAR_SIZE} />{/snippet}
            {#snippet trailing()}<MemberRoleChip label={t("contacts.role.owner")} />{/snippet}
          </MemberRow>
          {#each currentMembers.people as membership (membership.participant.id)}
            <MemberRow name={membership.participant.displayName}>
              {#snippet avatar()}<ChatParticipantAvatar participant={membership.participant} size={MEMBER_AVATAR_SIZE} />{/snippet}
              {#snippet trailing()}
                <MemberRoleSelect ariaLabel={t("contacts.role.label")} />
                <button type="button" class="dialog-icon-button control-unavailable" aria-label={t("chat.organization.removePerson", membership.participant.displayName)} aria-disabled="true"><X size={14} /></button>
              {/snippet}
            </MemberRow>
          {/each}
        </ul>
        <span class="member-group-label">{t("chat.organization.agents")}</span>
        <ul class="member-list">
          {#each memberAgents as agent (agent.participant.id)}
            <MemberRow name={agent.participant.displayName} detail={agent.configurationState === "healthy" ? t("chat.organization.aiAgent") : t("chat.organization.needsSetup")}>
              {#snippet avatar()}<ChatParticipantAvatar participant={agent.participant} {agent} size={MEMBER_AVATAR_SIZE} />{/snippet}
              {#snippet trailing()}
                {#if currentAgentIds.has(agent.participant.id)}
                  <button type="button" class="dialog-icon-button" aria-label={t("chat.organization.accessSettings", agent.participant.displayName)} onclick={configureMember}><Settings2 size={14} /></button>
                {/if}
                <button type="button" class="dialog-icon-button" aria-label={t("chat.organization.removeAgent", agent.participant.displayName)} disabled={removalBusy} onclick={() => void removeMember(agent.participant.id)}><X size={14} /></button>
              {/snippet}
            </MemberRow>
          {/each}
        </ul>
      </div>

      {#if error}<Notice class="text-panel-detail" message={error} />{/if}
    </div>

    <footer class="flex justify-end gap-2 px-5 pt-2 pb-4">
      <button type="button" class="chat-secondary-button" onclick={onCancel}>{t("common.cancel")}</button>
      <button type="submit" class="chat-primary-button" disabled={saving || !name.trim()}>{#if saving}<LoaderCircle size={14} class="animate-spin" />{/if}{currentChannel ? t("common.save") : t("chat.channels.create")}</button>
    </footer>
  </form>
  </div>
</div>

{#if pickerOpen}
  <ParticipantPicker
    anchor={addMembersButton}
    title={t("contacts.picker.title")}
    agents={chat.agents}
    memberAgentIds={selectedAgentIds}
    includeAgents
    space={{ kind: "channel", name: name.trim() }}
    onConfirm={addMembers}
    onClose={() => { pickerOpen = false; }}
    onNewAgent={setUpProvider}
  />
{/if}

{#if removal && removalParticipant}
  <ConfirmDialog
    title={t("chat.organization.removeAgentTitle", removalParticipant.displayName)}
    message={t("chat.organization.removeAgentImpact", removal.preview.activeAssignmentCount, removal.preview.activeAuthorizationCount)}
    confirmLabel={t("chat.organization.removeAgentConfirm")}
    cancelLabel={t("common.cancel")}
    onConfirm={() => { if (removal) dropMember(removal.agentId); removal = null; }}
    onCancel={() => { removal = null; }}
  />
{/if}

<style>
  .channel-dialog { max-height: min(40rem, 100%); }
  .field-label { color: var(--muted-foreground); font-size: var(--panel-detail-font-size); font-weight: 500; line-height: 1.4; }
  .dialog-icon-button { display: grid; width: 1.75rem; height: 1.75rem; flex: 0 0 auto; place-items: center; border-radius: 0.375rem; color: var(--muted-foreground); }
  .dialog-icon-button:hover:not(:disabled) { background: var(--accent); color: var(--foreground); }
  .dialog-icon-button:disabled { opacity: 0.45; }
  .member-list { display: grid; }
  .member-list:empty { display: none; }
  .member-group-label { padding-inline: 0.375rem; color: var(--muted-foreground); font-size: var(--panel-detail-font-size); font-weight: 500; line-height: 1.4; }
  .member-list + .member-group-label { margin-top: 0.25rem; }
  .member-add-button { display: inline-flex; min-height: 1.75rem; align-items: center; gap: 0.3rem; border-radius: 0.375rem; padding-inline: 0.5rem; color: var(--muted-foreground); font-size: var(--panel-detail-font-size); font-weight: 500; }
  .member-add-button:hover { background: var(--accent); color: var(--foreground); }
  @media (pointer: coarse) { .dialog-icon-button { width: 2.5rem; height: 2.5rem; } .member-add-button { min-height: 2.5rem; } }
</style>
