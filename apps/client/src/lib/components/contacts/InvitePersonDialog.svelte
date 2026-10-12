<script lang="ts">
  import { untrack } from "svelte";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import UserRoundPlus from "@lucide/svelte/icons/user-round-plus";
  import Checkbox from "$lib/components/ui/Checkbox.svelte";
  import Select from "$lib/components/ui/Select.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getChat } from "$lib/stores/chat.svelte";
  import { getProjects } from "$lib/stores/projects.svelte";
  import { overflowTooltip } from "$lib/utils/overflow-tooltip";
  import AddContactDialog from "./AddContactDialog.svelte";
  import {
    MEMBER_CAPABILITIES,
    MEMBER_CAPABILITY_GROUPS,
    INVITATION_HISTORY_BOUNDARIES,
    INVITABLE_MEMBER_ROLES,
    capabilitiesForRole,
    type MemberCapability,
    type InvitationHistoryBoundary,
    type MemberRole,
    type InvitationSpaceSummary,
  } from "./model";
  import ContactsDialog from "./ContactsDialog.svelte";
  import InvitationSpacePicker from "./InvitationSpacePicker.svelte";
  import { summarizeSpaceSelection, type InvitationSpaceNode, type InvitationSpaceSummaryEntry } from "./space-selection";

  /**
   * Invitation dialog shared by channels, projects, groups, pages, and events. Groups, projects, and channels are
   * chosen in the cascading space picker; a page, event, or conversation is fixed by the opener.
   */
  let {
    space = null,
    showHistory = true,
    onClose,
  }: {
    space?: InvitationSpaceSummary | null;
    showHistory?: boolean;
    onClose: () => void;
  } = $props();

  const ADD_CONTACT_OPTION = "add-contact";
  const SPACE_PATH_SEPARATOR = " / ";
  const { t } = getLocalization();
  const projects = getProjects();
  const chat = getChat();

  const tree = $derived.by((): InvitationSpaceNode[] => {
    const groups = projects.visibleGroups();
    const groupIds = new Set(groups.map((group) => group.id));
    const activeProjects = projects.projects.filter((project) => project.status === "active" && groupIds.has(project.groupId));
    const projectIds = new Set(activeProjects.map((project) => project.id));
    return [
      ...groups.map((group): InvitationSpaceNode => ({ id: group.id, kind: "group", name: group.name, parentId: null })),
      ...activeProjects.map((project): InvitationSpaceNode => ({ id: project.id, kind: "project", name: project.name, parentId: project.groupId })),
      ...chat.activeChannels
        .filter((channel) => channel.archivedAt === null && projectIds.has(channel.projectId))
        .map((channel): InvitationSpaceNode => ({ id: channel.id, kind: "channel", name: channel.name, parentId: channel.projectId })),
    ];
  });
  const pickableSpace = $derived(space?.id !== undefined && tree.some((node) => node.id === space.id));
  const fixedSpace = $derived(space && !pickableSpace ? space : null);

  let selectedSpaces = $state<ReadonlySet<string>>(new Set());
  let spacesAction = $state<HTMLButtonElement | null>(null);
  let spacePickerOpen = $state(false);
  let role = $state<MemberRole>("member");
  let history = $state<InvitationHistoryBoundary>("fromAcceptance");
  let capabilities = $state<ReadonlySet<MemberCapability>>(capabilitiesForRole("member"));
  let message = $state("");
  let addContactOpen = $state(false);

  untrack(() => {
    if (space?.id !== undefined && pickableSpace) selectedSpaces = new Set([space.id]);
  });

  const spaceSummary = $derived(summarizeSpaceSelection(tree, selectedSpaces));
  /** One path per covered space, in the same shape as the picker's search results, so the summary reads at the dialog's size. */
  const spaceLines = $derived.by((): string[] => {
    if (fixedSpace) return [[t(`contacts.space.${fixedSpace.kind}`), fixedSpace.name].join(SPACE_PATH_SEPARATOR)];
    if (spaceSummary.length === 0) return [t("contacts.invite.noSpaces")];
    return spaceSummary.flatMap((group) => {
      if (group.whole) return [[group.node.name, ...(group.childCount > 0 ? [wholeLabel(group)] : [])].join(SPACE_PATH_SEPARATOR)];
      return group.children.flatMap((project) => {
        const projectPath = [group.node.name, project.node.name];
        if (project.whole) return [[...projectPath, ...(project.childCount > 0 ? [wholeLabel(project)] : [])].join(SPACE_PATH_SEPARATOR)];
        return project.children.map((channel) => [...projectPath, `#${channel.node.name}`].join(SPACE_PATH_SEPARATOR));
      });
    });
  });
  const personOptions = $derived([{ value: ADD_CONTACT_OPTION, label: t("contacts.invite.addContact") }]);
  const roleOptions = $derived(INVITABLE_MEMBER_ROLES.map((value) => ({ value, label: t(`contacts.role.${value}`) })));
  const historyOptions = $derived(INVITATION_HISTORY_BOUNDARIES.map((value) => ({ value, label: t(`contacts.history.${value}`) })));

  function isRole(value: string): value is MemberRole {
    return (INVITABLE_MEMBER_ROLES as readonly string[]).includes(value);
  }

  function isHistory(value: string): value is InvitationHistoryBoundary {
    return (INVITATION_HISTORY_BOUNDARIES as readonly string[]).includes(value);
  }

  function changeRole(value: string): void {
    if (!isRole(value)) return;
    role = value;
    capabilities = capabilitiesForRole(value);
  }

  function toggleCapability(capability: MemberCapability, checked: boolean): void {
    const next = new Set(capabilities);
    if (checked) next.add(capability);
    else next.delete(capability);
    capabilities = next;
  }

  function wholeLabel(entry: InvitationSpaceSummaryEntry): string {
    return entry.node.kind === "group" ? t("contacts.invite.allProjects") : t("contacts.invite.allChannels");
  }

  function closeSpacePicker(): void {
    spacePickerOpen = false;
    spacesAction?.focus();
  }
</script>

<ContactsDialog
  title={t("contacts.invite.title")}
  cancelLabel={t("contacts.invite.cancel")}
  confirmLabel={t("contacts.invite.send")}
  {onClose}
>
  <!-- Rows carry their own inline padding, so the negative margin keeps labels on the title's left edge. -->
  <div class="-mx-1 flex flex-col gap-2">
    <Select
      label={t("contacts.invite.person")}
      description={t("contacts.invite.noContacts")}
      value=""
      triggerLabel={t("contacts.invite.personPlaceholder")}
      options={personOptions}
      showActiveCheck={false}
      onChange={(value) => { if (value === ADD_CONTACT_OPTION) addContactOpen = true; }}
    >
      {#snippet leading(value: string)}
        {#if value === ADD_CONTACT_OPTION}<UserRoundPlus size={14} class="shrink-0 text-muted-foreground" />{/if}
      {/snippet}
    </Select>

    <div class="flex flex-col gap-1 px-1 py-1">
      <div class="flex min-h-7 items-center justify-between gap-4">
        <span class="text-[0.866667rem] text-foreground">{t("contacts.invite.spaces")}</span>
        {#if !fixedSpace}
          <button
            bind:this={spacesAction}
            type="button"
            class="flex h-7 w-44 max-w-full items-center justify-between gap-2 rounded-md border border-border bg-transparent px-2.5 text-[0.8rem] font-medium text-foreground transition-colors hover:bg-accent"
            aria-haspopup="dialog"
            aria-expanded={spacePickerOpen}
            onclick={() => { spacePickerOpen = true; }}
          >
            <span class="truncate">{t("contacts.invite.editSpaces")}</span>
            <ChevronRight size={13} strokeWidth={2} class="shrink-0 text-muted-foreground" />
          </button>
        {/if}
      </div>
      <ul class="grid gap-0.5 text-[0.8rem] text-muted-foreground">
        {#each spaceLines as line (line)}
          <li class="truncate" use:overflowTooltip={line}>{line}</li>
        {/each}
      </ul>
    </div>

    <Select
      label={t("contacts.role.label")}
      value={role}
      options={roleOptions}
      onChange={changeRole}
    />

    {#if role === "custom"}
      {#each MEMBER_CAPABILITY_GROUPS as group (group)}
        <div class="px-1 pt-1 text-[0.733333rem] font-medium text-muted-foreground">{t(`contacts.capability.group.${group}`)}</div>
        {#each MEMBER_CAPABILITIES.filter((capability) => capability.group === group) as capability (capability.id)}
          <label class="flex cursor-pointer items-center justify-between gap-4 px-1 py-1">
            <span class="text-[0.866667rem] text-foreground">{t(`contacts.capability.${capability.id}`)}</span>
            <Checkbox checked={capabilities.has(capability.id)} onChange={(checked) => toggleCapability(capability.id, checked)} />
          </label>
        {/each}
      {/each}
    {/if}

    {#if showHistory}
      <Select
        label={t("contacts.history.label")}
        value={history}
        options={historyOptions}
        onChange={(value) => { if (isHistory(value)) history = value; }}
      />
    {/if}

    <label class="flex flex-col gap-1.5 px-1 py-1">
      <span class="text-[0.866667rem] text-foreground">{t("contacts.invite.message")}</span>
      <textarea bind:value={message} class="field min-h-20 resize-none text-[0.8rem]" placeholder={t("contacts.invite.messagePlaceholder")} maxlength="500" rows="3"></textarea>
    </label>
  </div>
</ContactsDialog>

{#if spacePickerOpen && spacesAction}
  <InvitationSpacePicker anchor={spacesAction} {tree} selected={selectedSpaces} onSelectionChange={(next) => { selectedSpaces = next; }} onClose={closeSpacePicker} />
{/if}

{#if addContactOpen}
  <AddContactDialog layer="second" onClose={() => { addContactOpen = false; }} />
{/if}
