<script lang="ts">
  import { untrack } from "svelte";
  import Pencil from "@lucide/svelte/icons/pencil";
  import Settings2 from "@lucide/svelte/icons/settings-2";
  import UserRoundPlus from "@lucide/svelte/icons/user-round-plus";
  import X from "@lucide/svelte/icons/x";
  import * as chatApi from "$lib/api/chat";
  import type { ChatChannelRead } from "$lib/chat/contracts";
  import { applyChannelMemberChanges } from "$lib/chat/teammates/channel-member-changes";
  import { summarizeChannelMembers } from "$lib/chat/teammates/channel-members";
  import ChatParticipantAvatar from "$lib/components/chat/identity/ChatParticipantAvatar.svelte";
  import { floatPanel } from "$lib/components/contacts/float-panel";
  import LocalPersonAvatar from "$lib/components/contacts/LocalPersonAvatar.svelte";
  import ParticipantPicker from "$lib/components/contacts/ParticipantPicker.svelte";
  import MemberRow from "$lib/components/contacts/MemberRow.svelte";
  import MemberRoleChip from "$lib/components/contacts/MemberRoleChip.svelte";
  import MemberRoleSelect from "$lib/components/contacts/MemberRoleSelect.svelte";
  import ProjectPickerMobileDialog from "$lib/components/projects/pickers/ProjectPickerMobileDialog.svelte";
  import { FLOATING_WIDTH } from "$lib/components/ui/floating-width";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { useChatPaneEvents } from "$lib/chat/pane-events";
  import { useChat } from "$lib/stores/chat.svelte";
  import { getPreferences } from "$lib/stores/preferences.svelte";
  import { getSettingsLauncher } from "$lib/stores/settings-launcher.svelte";
  import { cn } from "$lib/utils";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";
  import { CHAT_OPEN_MEMBERS_EVENT, type ChatOpenMembersDetail } from "./members-panel-events";
  import Notice from "$lib/components/ui/Notice.svelte";

  /**
   * Read-only view of who is in a channel, opened from the header. Adding members is possible here because it
   * needs no channel permission beyond membership; renaming and the topic stay in the channel dialog.
   */
  let {
    anchor = null,
    channel,
    mobileLayout = false,
    initialPickerOpen = false,
    onClose,
  }: {
    anchor?: HTMLElement | null;
    channel: ChatChannelRead;
    mobileLayout?: boolean;
    /** Opens the member picker at once, for a panel restored after a detour through Settings. */
    initialPickerOpen?: boolean;
    onClose: () => void;
  } = $props();

  const AVATAR_SIZE = 26;
  const ACTION_ICON_SIZE = 13;
  const ACTION_ICON_STROKE = 1.6;
  const chat = useChat();
  const paneEvents = useChatPaneEvents();
  const preferences = getPreferences();
  const settingsLauncher = getSettingsLauncher();
  const { t } = getLocalization();

  let addButton = $state<HTMLButtonElement | null>(null);
  let pickerOpen = $state(untrack(() => initialPickerOpen));
  let error = $state<string | null>(null);

  const summary = $derived(summarizeChannelMembers(channel.memberships));
  const teammates = $derived(chat.teammates.filter((teammate) => summary.teammateIds.has(teammate.participant.id)));
  const localName = $derived(preferences.profileDisplayName.trim() || t("contacts.you"));
  const localDetail = $derived(preferences.profileDisplayName.trim() ? t("contacts.you") : null);

  /** Footer actions share the Notes navigator layout: equal widths, centered, split by a short divider. */
  function footerActionClass(mobile: boolean): string {
    return cn(
      "flex min-w-0 flex-1 items-center justify-center gap-1.5 px-1 text-popover-foreground transition-colors hover:bg-accent hover:text-accent-foreground",
      mobile ? "min-h-12 rounded-xl text-sm active:bg-accent" : "min-h-8 rounded-floating-item",
    );
  }

  function configureTeammate(participantId: string): void {
    onClose();
    paneEvents.dispatchEvent(new CustomEvent("ganbaru-ai:chat-configure-teammate", { detail: { participantId, channelId: channel.id } }));
  }

  function editChannel(): void {
    onClose();
    paneEvents.dispatchEvent(new CustomEvent("ganbaru-ai:chat-edit-channel", { detail: { channelId: channel.id } }));
  }

  /** Settings covers the panel, so it closes now and asks the header to reopen it with the picker once Settings is gone. */
  function createTeammate(): void {
    const channelId = channel.id;
    onClose();
    settingsLauncher.open("chat", {
      chatSubsection: "teammates",
      chatChannelId: channelId,
      chatCreateTeammate: true,
      onClosed: () => paneEvents.dispatchEvent(new CustomEvent(CHAT_OPEN_MEMBERS_EVENT, { detail: { channelId, addMembers: true } satisfies ChatOpenMembersDetail })),
    });
  }

  async function addTeammates(teammateIds: string[]): Promise<void> {
    error = null;
    try {
      const next = new Set([...summary.teammateIds, ...teammateIds]);
      if (await applyChannelMemberChanges(summary.teammateIds, next, channel.id)) {
        const saved = await chatApi.readChatChannel(channel.id);
        chat.activeChannels = chat.activeChannels.map((entry) => entry.id === saved.id ? saved : entry);
        await chat.refreshTeammates();
      }
    } catch (cause: unknown) {
      error = cause instanceof Error ? cause.message : String(cause);
    }
  }
</script>

{#snippet content(mobile: boolean)}
  <header class="flex items-start gap-2 px-3 pt-3 pb-2" class:mobile-header={mobile}>
    <div class="min-w-0 flex-1">
      <h2 class="truncate text-[0.933333rem] font-semibold">#{channel.name}</h2>
      <p class="text-panel-detail text-muted-foreground">{t("chat.organization.membersPanel", summary.count)}</p>
      {#if channel.topic}<p class="mt-1 line-clamp-2 text-panel-detail text-muted-foreground">{channel.topic}</p>{/if}
    </div>
    {#if mobile}
      <button type="button" class="members-close" aria-label={t("chat.organization.closeMembers")} onclick={onClose}><X size={18} /></button>
    {/if}
  </header>

  <div class="members-body min-h-0 flex-1 overflow-y-auto px-1.5 pb-1" class:capped={!mobile} use:scrollEdgeFadeAction>
    {#if teammates.length > 0}<div class="menu-label">{t("chat.organization.people")}</div>{/if}
    <ul class="grid">
      <MemberRow name={localName} detail={localDetail}>
        {#snippet avatar()}<LocalPersonAvatar size={AVATAR_SIZE} />{/snippet}
        {#snippet trailing()}<MemberRoleChip label={t("contacts.role.owner")} />{/snippet}
      </MemberRow>
      {#each summary.people as membership (membership.participant.id)}
        <MemberRow name={membership.participant.displayName}>
          {#snippet avatar()}<ChatParticipantAvatar participant={membership.participant} size={AVATAR_SIZE} />{/snippet}
          {#snippet trailing()}<MemberRoleSelect ariaLabel={t("contacts.role.label")} />{/snippet}
        </MemberRow>
      {/each}
    </ul>
    {#if teammates.length > 0}
      <div class="menu-label">{t("chat.organization.teammates")}</div>
      <ul class="grid">
        {#each teammates as teammate (teammate.participant.id)}
          <MemberRow name={teammate.participant.displayName} detail={teammate.configurationState === "healthy" ? teammate.role || t("chat.organization.aiTeammate") : t("chat.organization.needsSetup")}>
            {#snippet avatar()}<ChatParticipantAvatar participant={teammate.participant} {teammate} size={AVATAR_SIZE} />{/snippet}
            {#snippet trailing()}
              <button type="button" class="members-icon-button" aria-label={t("chat.organization.accessSettings", teammate.participant.displayName)} onclick={() => configureTeammate(teammate.participant.id)}><Settings2 size={14} /></button>
            {/snippet}
          </MemberRow>
        {/each}
      </ul>
    {/if}
    {#if error}<Notice class="px-2 pb-1.5 text-panel-detail" message={error} />{/if}
  </div>

  <footer class={cn("relative z-10 shrink-0 bg-popover", mobile ? "p-2" : "p-1.5")}>
    <div class="pointer-events-none absolute inset-x-1.5 top-0 border-t border-border/70" aria-hidden="true"></div>
    <div class={cn("flex items-center", mobile ? "min-h-12" : "min-h-8")}>
      <button bind:this={addButton} type="button" class={footerActionClass(mobile)} aria-haspopup="dialog" aria-expanded={pickerOpen} onclick={() => { pickerOpen = true; }}><UserRoundPlus size={ACTION_ICON_SIZE} strokeWidth={ACTION_ICON_STROKE} /><span class="truncate">{t("chat.organization.addMembers")}</span></button>
      <div class="mx-1 h-5 border-l border-border/70" aria-hidden="true"></div>
      <button type="button" class={footerActionClass(mobile)} onclick={editChannel}><Pencil size={ACTION_ICON_SIZE} strokeWidth={ACTION_ICON_STROKE} /><span class="truncate">{t("chat.organization.editChannel")}</span></button>
    </div>
  </footer>
{/snippet}

{#if mobileLayout}
  <ProjectPickerMobileDialog label={t("chat.organization.membersPanelTitle")} closeLabel={t("chat.organization.closeMembers")} {onClose}>
    <div class="surface-floating flex h-full min-h-0 flex-col overflow-hidden rounded-2xl" data-floating-root data-chat-members-panel>
      {@render content(true)}
    </div>
  </ProjectPickerMobileDialog>
{:else if anchor}
  <div
    class="surface-floating fixed z-80 flex flex-col overflow-hidden outline-none"
    role="dialog"
    aria-label={t("chat.organization.membersPanelTitle")}
    tabindex="-1"
    data-floating-root
    data-app-floating-surface
    data-chat-members-panel
    use:floatPanel={{ anchor, width: FLOATING_WIDTH.lg, horizontalAlign: "end", dismissEnabled: !pickerOpen, onDismiss: onClose }}
  >
    {@render content(false)}
  </div>
{/if}

{#if pickerOpen}
  <ParticipantPicker
    anchor={addButton}
    title={t("contacts.picker.title")}
    teammates={chat.teammates}
    memberTeammateIds={summary.teammateIds}
    includeTeammates
    space={{ kind: "channel", id: channel.id, name: channel.name }}
    onConfirm={addTeammates}
    onClose={() => { pickerOpen = false; }}
    onNewTeammate={createTeammate}
  />
{/if}

<style>
  .members-body.capped { max-height: min(24rem, 60vh); }
  .mobile-header { min-height: 3.5rem; border-bottom: 1px solid color-mix(in srgb, var(--border) 70%, transparent); }
  .members-close { display: grid; min-width: 3rem; min-height: 3rem; flex: 0 0 auto; place-items: center; border-radius: 0.75rem; }
  .members-close:active { background: var(--accent); }
  .members-icon-button { display: grid; width: 1.75rem; height: 1.75rem; place-items: center; border-radius: 0.375rem; color: var(--muted-foreground); }
  .members-icon-button:hover { background: var(--accent); color: var(--foreground); }
  @media (pointer: coarse) { .members-icon-button { width: 2.5rem; height: 2.5rem; } }
</style>
