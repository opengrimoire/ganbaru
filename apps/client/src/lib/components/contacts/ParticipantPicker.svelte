<script lang="ts">
  import Bot from "@lucide/svelte/icons/bot";
  import MailPlus from "@lucide/svelte/icons/mail-plus";
  import UserRoundPlus from "@lucide/svelte/icons/user-round-plus";
  import X from "@lucide/svelte/icons/x";
  import type { ChatAiTeammateRead } from "$lib/chat/contracts";
  import ChatParticipantAvatar from "$lib/components/chat/identity/ChatParticipantAvatar.svelte";
  import ProjectPickerMobileDialog from "$lib/components/projects/pickers/ProjectPickerMobileDialog.svelte";
  import Checkbox from "$lib/components/ui/Checkbox.svelte";
  import { FLOATING_WIDTH } from "$lib/components/ui/floating-width";
  import SearchField from "$lib/components/ui/SearchField.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { BUILD_PLATFORM_PROFILE } from "$lib/platform";
  import { cn } from "$lib/utils";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";
  import AddContactDialog from "./AddContactDialog.svelte";
  import { floatPanel } from "./float-panel";
  import InvitePersonDialog from "./InvitePersonDialog.svelte";
  import type { InvitationSpaceSummary } from "./model";

  /**
   * The one member picker: contacts and teammates in one searchable list with multi-selection chips, the way
   * messaging apps build a group. Each group label carries its own creation action and invitations sit beside the
   * title, so the list holds only participants. Desktop anchors it to its trigger; the mobile shell shows it as a sheet.
   */
  let {
    anchor = null,
    title,
    teammates = [],
    memberTeammateIds = new Set<string>(),
    includeContacts = true,
    includeTeammates = false,
    space = null,
    horizontalAlign = "end",
    confirmLabel = null,
    onConfirm,
    onClose,
    onNewTeammate,
  }: {
    /** Trigger the desktop panel anchors to; the mobile sheet ignores it. */
    anchor?: HTMLElement | null;
    title: string;
    /** Every teammate of the vault; members of the target space are shown as already added. */
    teammates?: readonly ChatAiTeammateRead[];
    memberTeammateIds?: ReadonlySet<string>;
    includeContacts?: boolean;
    includeTeammates?: boolean;
    /** Enables "Invite someone" for spaces that accept invitations. */
    space?: InvitationSpaceSummary | null;
    horizontalAlign?: "start" | "end";
    confirmLabel?: string | null;
    /** Receives the selected teammate IDs; when absent the confirmation stays unavailable. */
    onConfirm?: (teammateIds: string[]) => void | Promise<void>;
    onClose: () => void;
    onNewTeammate?: () => void;
  } = $props();

  const AVATAR_SIZE = 22;
  const CHIP_AVATAR_SIZE = 18;
  const HEADING_ICON_SIZE = 13;
  const HEADING_ICON_STROKE = 1.6;
  const { t } = getLocalization();
  const mobileShell = BUILD_PLATFORM_PROFILE.shell === "mobile";

  let query = $state("");
  let selectedIds = $state<ReadonlySet<string>>(new Set());
  let inviteOpen = $state(false);
  let addContactOpen = $state(false);
  let confirming = $state(false);

  const normalizedQuery = $derived(query.trim().toLocaleLowerCase());
  const candidates = $derived(teammates.filter((teammate) => !memberTeammateIds.has(teammate.participant.id)));
  const matchingCandidates = $derived(candidates.filter((teammate) =>
    !normalizedQuery
    || teammate.participant.displayName.toLocaleLowerCase().includes(normalizedQuery)
    || teammate.role.toLocaleLowerCase().includes(normalizedQuery)));
  const selectedTeammates = $derived(candidates.filter((teammate) => selectedIds.has(teammate.participant.id)));
  const nestedOpen = $derived(inviteOpen || addContactOpen);
  const confirmText = $derived(selectedIds.size > 0 ? t("contacts.picker.confirmCount", selectedIds.size) : (confirmLabel ?? t("contacts.picker.confirm")));

  function teammateDetail(teammate: ChatAiTeammateRead): string {
    if (teammate.configurationState !== "healthy") return t("chat.organization.needsSetup");
    return teammate.role || t("chat.organization.aiTeammate");
  }

  function setSelected(teammateId: string, selected: boolean): void {
    const next = new Set(selectedIds);
    if (selected) next.add(teammateId);
    else next.delete(teammateId);
    selectedIds = next;
  }

  async function confirm(): Promise<void> {
    if (!onConfirm || selectedIds.size === 0 || confirming) return;
    confirming = true;
    try {
      await onConfirm([...selectedIds]);
    } finally {
      confirming = false;
    }
  }
</script>

{#snippet headingAction(label: string, icon: typeof Bot, mobileLayout: boolean, onclick: () => void)}
  {@const Icon = icon}
  <button type="button" class={cn("picker-heading-action", mobileLayout && "mobile")} {onclick}>
    <Icon size={HEADING_ICON_SIZE} strokeWidth={HEADING_ICON_STROKE} />
    <span>{label}</span>
  </button>
{/snippet}

{#snippet content(mobileLayout: boolean)}
  <div class={cn("flex items-center justify-between gap-2", mobileLayout ? "min-h-14 border-b border-border/70 px-2" : "px-3 pt-2.5 pb-1.5")}>
    <h2 class={cn("min-w-0 flex-1 truncate", mobileLayout ? "px-2 text-base font-semibold" : "text-panel-detail font-medium text-muted-foreground")}>{title}</h2>
    {#if space}
      {@render headingAction(t("contacts.picker.inviteSomeone"), MailPlus, mobileLayout, () => { inviteOpen = true; })}
    {/if}
    {#if mobileLayout}
      <button type="button" class="flex min-h-12 min-w-12 items-center justify-center rounded-xl active:bg-accent" aria-label={t("contacts.picker.close")} onclick={onClose}><X size={18} /></button>
    {/if}
  </div>
  <div class={cn("shrink-0 px-2", mobileLayout ? "pt-2 pb-1" : "pb-1")}>
    <SearchField bind:value={query} inline label={includeTeammates ? t("contacts.picker.search") : t("contacts.picker.searchContacts")} clearLabel={t("contacts.picker.clearSearch")} focusOnMount={!mobileLayout} />
  </div>
  {#if selectedTeammates.length > 0}
    <div class="flex flex-wrap gap-1 px-2 pb-1">
      {#each selectedTeammates as teammate (teammate.participant.id)}
        <span class="inline-flex max-w-full items-center gap-1 rounded-full border border-border py-0.5 pr-1 pl-0.5 text-panel-detail">
          <ChatParticipantAvatar participant={teammate.participant} {teammate} size={CHIP_AVATAR_SIZE} />
          <span class="min-w-0 truncate">{teammate.participant.displayName}</span>
          <button type="button" class="grid size-4 place-items-center rounded-full text-muted-foreground hover:bg-accent hover:text-foreground" aria-label={t("contacts.picker.remove", teammate.participant.displayName)} onclick={() => setSelected(teammate.participant.id, false)}><X size={11} /></button>
        </span>
      {/each}
    </div>
  {/if}
  <!-- Group labels stay pinned while a long list scrolls, so each group keeps its action within reach. -->
  <div use:scrollEdgeFadeAction class={cn("min-h-0 flex-1 overflow-y-auto px-1.5 pb-1", mobileLayout ? "overscroll-contain" : "picker-list")}>
    {#if includeContacts}
      <div class="picker-group-label menu-label">
        <span class="truncate">{t("contacts.picker.contacts")}</span>
        {@render headingAction(t("contacts.picker.addContact"), UserRoundPlus, mobileLayout, () => { addContactOpen = true; })}
      </div>
      <p class="picker-empty">{t("contacts.picker.noContacts")}</p>
    {/if}
    {#if includeTeammates}
      <div class="picker-group-label menu-label">
        <span class="truncate">{t("contacts.picker.teammates")}</span>
        {#if onNewTeammate}{@render headingAction(t("contacts.picker.newTeammate"), Bot, mobileLayout, onNewTeammate)}{/if}
      </div>
      {#if teammates.length === 0}
        <p class="picker-empty">{t("contacts.picker.noTeammates")}</p>
      {:else if candidates.length === 0}
        <p class="picker-empty">{t("contacts.picker.allAdded")}</p>
      {:else if matchingCandidates.length === 0}
        <p class="picker-empty">{t("contacts.picker.noMatches")}</p>
      {/if}
      {#each matchingCandidates as teammate (teammate.participant.id)}
        <label class="menu-item cursor-pointer gap-2.5">
          <Checkbox checked={selectedIds.has(teammate.participant.id)} onChange={(checked) => setSelected(teammate.participant.id, checked)} />
          <ChatParticipantAvatar participant={teammate.participant} {teammate} size={AVATAR_SIZE} />
          <span class="min-w-0 flex-1 truncate">{teammate.participant.displayName}</span>
          <span class="max-w-[45%] truncate text-panel-detail text-muted-foreground">{teammateDetail(teammate)}</span>
        </label>
      {/each}
    {/if}
  </div>
  <div class={cn("relative z-10 flex shrink-0 items-center justify-end gap-2 bg-popover px-3", mobileLayout ? "py-3" : "py-2")}>
    <div class="pointer-events-none absolute inset-x-1.5 top-0 border-t border-border/70" aria-hidden="true"></div>
    {#if selectedIds.size > 0}
      <span class="min-w-0 flex-1 truncate text-panel-detail text-muted-foreground">{t("contacts.picker.selected", selectedIds.size)}</span>
    {/if}
    <button type="button" class="picker-button" onclick={onClose}>{t("contacts.picker.cancel")}</button>
    {#if onConfirm}
      <button type="button" class="picker-button primary" disabled={selectedIds.size === 0 || confirming} onclick={() => void confirm()}>{confirmText}</button>
    {:else}
      <button type="button" class="picker-button primary control-unavailable" aria-disabled="true">{confirmText}</button>
    {/if}
  </div>
{/snippet}

{#if mobileShell}
  <ProjectPickerMobileDialog label={title} closeLabel={t("contacts.picker.close")} {onClose}>
    <div class="surface-floating flex h-full min-h-0 flex-col overflow-hidden rounded-2xl" data-floating-root data-app-floating-surface>
      {@render content(true)}
    </div>
  </ProjectPickerMobileDialog>
{:else if anchor}
  <div
    use:floatPanel={{ anchor, width: FLOATING_WIDTH.lg, horizontalAlign, dismissEnabled: !nestedOpen, onDismiss: onClose }}
    class="surface-floating fixed z-80 flex flex-col overflow-hidden outline-none"
    role="dialog"
    aria-label={title}
    tabindex="-1"
    data-floating-root
    data-app-floating-surface
  >
    {@render content(false)}
  </div>
{/if}

{#if inviteOpen && space}
  <InvitePersonDialog {space} showHistory={space.kind === "channel" || space.kind === "project" || space.kind === "group"} onClose={() => { inviteOpen = false; }} />
{/if}
{#if addContactOpen}
  <AddContactDialog onClose={() => { addContactOpen = false; }} />
{/if}

<style>
  .picker-list { max-height: min(22rem, 55vh); }
  .picker-group-label { position: sticky; top: 0; z-index: 1; display: flex; align-items: center; justify-content: space-between; gap: 0.5rem; padding-block: 0.25rem; background: var(--popover); }
  .picker-empty { padding: 0.125rem 0.5rem 0.375rem; color: var(--muted-foreground); font-size: var(--panel-detail-font-size); }
  .picker-heading-action { display: inline-flex; min-height: 1.25rem; flex: 0 0 auto; align-items: center; gap: 0.3rem; border-radius: var(--floating-item-radius); padding-inline: 0.25rem; color: var(--muted-foreground); font-size: var(--panel-detail-font-size); font-weight: 500; white-space: nowrap; transition: color 120ms ease; }
  .picker-heading-action:hover { color: var(--foreground); }
  .picker-heading-action.mobile { min-height: 2.5rem; padding-inline: 0.5rem; font-size: 0.875rem; }
  .picker-heading-action.mobile:active { background: var(--accent); }
  .picker-button { display: inline-flex; height: 1.75rem; flex: 0 0 auto; align-items: center; justify-content: center; gap: 0.375rem; border: 1px solid var(--border); border-radius: 0.375rem; padding-inline: 0.625rem; color: var(--foreground); font-size: calc(0.8rem * var(--type-scale)); font-weight: 500; transition: background-color 120ms ease; }
  .picker-button:hover:not(:disabled) { background: var(--accent); }
  .picker-button.primary { border-color: transparent; background: var(--primary); color: var(--primary-foreground); }
  .picker-button.primary:hover:not(:disabled) { background: color-mix(in srgb, var(--primary) 90%, transparent); }
  .picker-button:disabled { cursor: not-allowed; opacity: 0.5; }
  @media (pointer: coarse) {
    .picker-button { height: 2.5rem; }
  }
</style>
