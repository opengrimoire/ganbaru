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
  import { applyChannelMemberChanges } from "$lib/chat/teammates/channel-member-changes";
  import { summarizeChannelMembers } from "$lib/chat/teammates/channel-members";
  import ChatParticipantAvatar from "$lib/components/chat/identity/ChatParticipantAvatar.svelte";
  import LocalPersonAvatar from "$lib/components/contacts/LocalPersonAvatar.svelte";
  import ParticipantPicker from "$lib/components/contacts/ParticipantPicker.svelte";
  import MemberRow from "$lib/components/contacts/MemberRow.svelte";
  import MemberRoleChip from "$lib/components/contacts/MemberRoleChip.svelte";
  import MemberRoleSelect from "$lib/components/contacts/MemberRoleSelect.svelte";
  import ConfirmDialog from "$lib/components/ui/ConfirmDialog.svelte";
  import Select from "$lib/components/ui/Select.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { activateModalFocus, activateModalKeyboardLayer, trapModalTabKey } from "$lib/modal-focus";
  import { getChat } from "$lib/stores/chat.svelte";
  import { getPreferences } from "$lib/stores/preferences.svelte";
  import { getProjects } from "$lib/stores/projects.svelte";
  import { getSettingsLauncher } from "$lib/stores/settings-launcher.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";

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

  const chat = getChat();
  const preferences = getPreferences();
  const projects = getProjects();
  const settingsLauncher = getSettingsLauncher();
  const { t } = getLocalization();
  const currentChannel = untrack(() => channel);
  const projectId = $derived(currentChannel?.projectId ?? projects.selectedProjectId ?? "");
  const sectionOptions = $derived([
    { value: "", label: t("chat.channels.defaultSection") },
    ...sections.map((section) => ({ value: section.id, label: section.name })),
  ]);
  const currentMembers = summarizeChannelMembers(currentChannel?.memberships ?? []);
  const currentTeammateIds = currentMembers.teammateIds;

  let dialog = $state<HTMLDivElement | null>(null);
  let nameInput = $state<HTMLInputElement | null>(null);
  let addMembersButton = $state<HTMLButtonElement | null>(null);
  let name = $state(currentChannel?.name ?? "");
  let topic = $state(currentChannel?.topic ?? "");
  let sectionId = $state(untrack(() => initialSectionId ?? ""));
  let selectedTeammateIds = $state<ReadonlySet<string>>(new Set(currentTeammateIds));
  let pickerOpen = $state(false);
  let saving = $state(false);
  let error = $state<string | null>(null);
  let removal = $state<{ teammateId: string; preview: ChatChannelMembershipRemovalPreview } | null>(null);
  let removalBusy = $state(false);

  const memberTeammates = $derived(chat.teammates.filter((teammate) => selectedTeammateIds.has(teammate.participant.id)));
  const localName = $derived(preferences.profileDisplayName.trim() || t("contacts.you"));
  const localDetail = $derived(preferences.profileDisplayName.trim() ? t("contacts.you") : null);
  const removalParticipant = $derived.by(() => {
    const pending = removal;
    if (!pending) return null;
    return chat.teammates.find((teammate) => teammate.participant.id === pending.teammateId)?.participant ?? null;
  });

  function addMembers(teammateIds: string[]): void {
    selectedTeammateIds = new Set([...selectedTeammateIds, ...teammateIds]);
  }

  /** Settings covers the dialog, so it closes now and comes back for the same channel once Settings is gone. */
  function createTeammate(): void {
    const channelId = currentChannel?.id;
    onCancel();
    settingsLauncher.open("chat", {
      chatSubsection: "teammates",
      chatChannelId: channelId,
      chatCreateTeammate: true,
      onClosed: () => {
        if (channelId) window.dispatchEvent(new CustomEvent("ganbaru-ai:chat-edit-channel", { detail: { channelId } }));
        else window.dispatchEvent(new Event("ganbaru-ai:chat-new-channel"));
      },
    });
  }

  function dropMember(teammateId: string): void {
    const next = new Set(selectedTeammateIds);
    next.delete(teammateId);
    selectedTeammateIds = next;
  }

  async function removeMember(teammateId: string): Promise<void> {
    if (!currentChannel || !currentTeammateIds.has(teammateId)) {
      dropMember(teammateId);
      return;
    }
    if (removalBusy) return;
    removalBusy = true;
    error = null;
    try {
      const access = await chatApi.readChatTeammateAccess(teammateId);
      const preview = await chatApi.previewChatChannelMembershipRemoval({
        teammateId,
        channelId: currentChannel.id,
        expectedAccessRevision: access.accessRevision,
      });
      if (preview.activeAssignmentCount === 0 && preview.activeAuthorizationCount === 0) dropMember(teammateId);
      else removal = { teammateId, preview };
    } catch (cause: unknown) {
      error = cause instanceof Error ? cause.message : String(cause);
    } finally {
      removalBusy = false;
    }
  }

  function configureMember(participantId: string): void {
    onCancel();
    window.dispatchEvent(new CustomEvent("ganbaru-ai:chat-configure-teammate", {
      detail: { participantId, channelId: currentChannel?.id },
    }));
  }

  async function save(): Promise<void> {
    if (!projectId || saving || !name.trim()) return;
    saving = true;
    error = null;
    try {
      let saved = currentChannel
        ? await chat.updateChannelDetails(currentChannel, name, topic)
        : await chat.createChannel({ id: `channel:${crypto.randomUUID()}`, projectId, name, topic });
      if (await applyChannelMemberChanges(currentTeammateIds, selectedTeammateIds, saved.id)) {
        saved = await chatApi.readChatChannel(saved.id);
        chat.activeChannels = chat.activeChannels.map((entry) => entry.id === saved.id ? saved : entry);
        await chat.refreshTeammates();
      }
      onSaved(saved, sectionId || null);
    } catch (cause: unknown) {
      error = cause instanceof Error ? cause.message : String(cause);
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
        <div class="field flex h-9 items-center gap-1 pl-2.5"><Hash size={14} class="shrink-0 text-muted-foreground" /><input bind:this={nameInput} class="field-bare min-w-0 flex-1" bind:value={name} maxlength="80" required disabled={currentChannel?.isDefault} /></div>
      </label>
      <label class="grid gap-1.5">
        <span class="field-label">{t("chat.channels.topic")}</span>
        <input class="field h-9" bind:value={topic} maxlength="250" placeholder={t("chat.channels.topicPlaceholder")} />
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
        <span class="member-group-label">{t("chat.organization.teammates")}</span>
        <ul class="member-list">
          {#each memberTeammates as teammate (teammate.participant.id)}
            <MemberRow name={teammate.participant.displayName} detail={teammate.configurationState === "healthy" ? teammate.role || t("chat.organization.aiTeammate") : t("chat.organization.needsSetup")}>
              {#snippet avatar()}<ChatParticipantAvatar participant={teammate.participant} {teammate} size={MEMBER_AVATAR_SIZE} />{/snippet}
              {#snippet trailing()}
                {#if currentTeammateIds.has(teammate.participant.id)}
                  <button type="button" class="dialog-icon-button" aria-label={t("chat.organization.accessSettings", teammate.participant.displayName)} onclick={() => configureMember(teammate.participant.id)}><Settings2 size={14} /></button>
                {/if}
                <button type="button" class="dialog-icon-button" aria-label={t("chat.organization.removeTeammate", teammate.participant.displayName)} disabled={removalBusy} onclick={() => void removeMember(teammate.participant.id)}><X size={14} /></button>
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
    teammates={chat.teammates}
    memberTeammateIds={selectedTeammateIds}
    includeTeammates
    space={{ kind: "channel", name: name.trim() }}
    onConfirm={addMembers}
    onClose={() => { pickerOpen = false; }}
    onNewTeammate={createTeammate}
  />
{/if}

{#if removal && removalParticipant}
  <ConfirmDialog
    title={t("chat.organization.removeTeammateTitle", removalParticipant.displayName)}
    message={t("chat.organization.removeTeammateImpact", removal.preview.activeAssignmentCount, removal.preview.activeAuthorizationCount)}
    confirmLabel={t("chat.organization.removeTeammateConfirm")}
    cancelLabel={t("common.cancel")}
    onConfirm={() => { if (removal) dropMember(removal.teammateId); removal = null; }}
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
