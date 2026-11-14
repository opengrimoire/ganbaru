<script lang="ts">
  import Pencil from "@lucide/svelte/icons/pencil";
  import Trash from "@lucide/svelte/icons/trash";
  import X from "@lucide/svelte/icons/x";
  import {
    createChatChannelNote,
    deleteChatChannelNote,
    listChatChannelNotes,
    updateChatChannelNote,
  } from "$lib/api/chat/channel-notes";
  import {
    channelNoteDraftProblem,
    channelNoteParticipantName,
    type ChannelNoteDraftProblem,
    type ChannelNoteNameSources,
  } from "$lib/chat/channel-notes/presentation";
  import {
    CHAT_MAX_ACTIVE_CHANNEL_NOTES,
    CHAT_MAX_CHANNEL_NOTE_CHARS,
    type ChatChannelId,
    type ChatChannelNoteRead,
    type ChatChannelRead,
  } from "$lib/chat/contracts";
  import { chatErrorMessage } from "$lib/chat/error-presentation";
  import { floatPanel } from "$lib/components/contacts/float-panel";
  import ProjectPickerMobileDialog from "$lib/components/projects/pickers/ProjectPickerMobileDialog.svelte";
  import { requestConfirmation } from "$lib/components/ui/confirmation.svelte";
  import { FLOATING_WIDTH } from "$lib/components/ui/floating-width";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { formatDateTime } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { useChat } from "$lib/stores/chat.svelte";
  import { getPreferences } from "$lib/stores/preferences.svelte";
  import { cn } from "$lib/utils";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";

  /**
   * The notes people keep for every teammate working in a channel, opened from the members panel. People
   * add, reword, and delete notes here; teammates only suggest them in their reply threads.
   */
  let {
    anchor = null,
    channel,
    mobileLayout = false,
    onClose,
  }: {
    anchor?: HTMLElement | null;
    channel: ChatChannelRead;
    mobileLayout?: boolean;
    onClose: () => void;
  } = $props();

  const ACTION_ICON_SIZE = 13;
  const chat = useChat();
  const preferences = getPreferences();
  const localization = getLocalization();
  const { t } = localization;

  let notes = $state<ChatChannelNoteRead[]>([]);
  let loaded = $state(false);
  let error = $state<string | null>(null);
  let pending = $state(false);
  let draft = $state("");
  let editingId = $state<string | null>(null);
  let editDraft = $state("");

  const nameSources = $derived<ChannelNoteNameSources>({
    participants: [
      ...channel.memberships.map((membership) => membership.participant),
      ...chat.teammates.map((teammate) => teammate.participant),
    ],
    localProfileDisplayName: preferences.profileDisplayName,
    localFallback: t("contacts.you"),
    unknownName: t("chat.channelNotes.unknownPerson"),
  });
  const addProblem = $derived(channelNoteDraftProblem(draft, notes.length, true));
  const editProblem = $derived(channelNoteDraftProblem(editDraft, notes.length, false));

  $effect(() => {
    const _version = chat.channelNotesVersion;
    void load(channel.id);
  });

  async function load(channelId: ChatChannelId): Promise<void> {
    try {
      const read = await listChatChannelNotes(channelId);
      if (channelId !== channel.id) return;
      notes = read;
      loaded = true;
    } catch (cause: unknown) {
      error = chatErrorMessage(cause, t("chatErrors.actions.notesAction"));
    }
  }

  function problemMessage(problem: ChannelNoteDraftProblem | null): string | null {
    if (problem === "limit") return t("chat.channelNotes.limitReached", CHAT_MAX_ACTIVE_CHANNEL_NOTES);
    if (problem === "too_long") return t("chat.channelNotes.tooLong", CHAT_MAX_CHANNEL_NOTE_CHARS);
    return null;
  }

  function noteTime(note: ChatChannelNoteRead): string {
    return formatDateTime(localization.locale, Date.parse(note.updatedAt), { dateStyle: "medium", timeStyle: "short" });
  }

  /** Runs one note command, then rereads the list and tells other panes and windows. */
  async function run(command: () => Promise<unknown>): Promise<boolean> {
    if (pending) return false;
    pending = true;
    error = null;
    const channelId = channel.id;
    try {
      await command();
      chat.channelNotesChanged(channelId);
      await load(channelId);
      return true;
    } catch (cause: unknown) {
      error = chatErrorMessage(cause, t("chatErrors.actions.notesAction"));
      return false;
    } finally {
      pending = false;
    }
  }

  async function addNote(): Promise<void> {
    if (addProblem !== null) return;
    const body = draft;
    if (await run(() => createChatChannelNote({ clientCommandId: crypto.randomUUID(), channelId: channel.id, body }))) {
      draft = "";
    }
  }

  function startEdit(note: ChatChannelNoteRead): void {
    editingId = note.id;
    editDraft = note.body;
    error = null;
  }

  async function saveEdit(note: ChatChannelNoteRead): Promise<void> {
    if (editProblem !== null) return;
    const body = editDraft;
    if (await run(() => updateChatChannelNote({
      clientCommandId: crypto.randomUUID(),
      noteId: note.id,
      expectedRevision: note.revision,
      body,
    }))) {
      editingId = null;
    }
  }

  async function deleteNote(note: ChatChannelNoteRead): Promise<void> {
    if (pending) return;
    const confirmed = await requestConfirmation({
      message: t("chat.channelNotes.deleteConfirm"),
      confirmLabel: t("chat.channelNotes.delete"),
      danger: true,
    });
    if (!confirmed) return;
    await run(() => deleteChatChannelNote({
      clientCommandId: crypto.randomUUID(),
      noteId: note.id,
      expectedRevision: note.revision,
    }));
  }

  function submitOnShortcut(event: KeyboardEvent, submit: () => Promise<void>): void {
    if (event.key === "Enter" && (event.ctrlKey || event.metaKey)) {
      event.preventDefault();
      void submit();
    }
  }
</script>

{#snippet content(mobile: boolean)}
  <header class="flex items-start gap-2 px-3 pt-3 pb-2" class:mobile-header={mobile}>
    <div class="min-w-0 flex-1">
      <h2 class="truncate text-[0.933333rem] font-semibold">{t("chat.channelNotes.title")}</h2>
      <p class="text-panel-detail text-muted-foreground" data-channel-notes-count>#{channel.name} · {t("chat.channelNotes.count", notes.length, CHAT_MAX_ACTIVE_CHANNEL_NOTES)}</p>
    </div>
    {#if mobile}
      <button type="button" class="notes-close" aria-label={t("chat.channelNotes.close")} onclick={onClose}><X size={18} /></button>
    {/if}
  </header>

  <div class="notes-body min-h-0 flex-1 overflow-y-auto px-1.5 pb-1" class:capped={!mobile} use:scrollEdgeFadeAction>
    {#if loaded && notes.length === 0}
      <p class="px-2 py-1.5 text-panel-detail text-muted-foreground" data-channel-notes-empty>{t("chat.channelNotes.empty")}</p>
    {/if}
    <ul class="grid gap-0.5">
      {#each notes as note (note.id)}
        {@const savedBy = channelNoteParticipantName(note.editedBy, nameSources)}
        {@const suggestedBy = channelNoteParticipantName(note.proposedBy, nameSources)}
        <li class="note-row" data-channel-note={note.id}>
          {#if editingId === note.id}
            <textarea
              class="field min-h-16 resize-none text-[0.8rem]"
              rows="3"
              bind:value={editDraft}
              aria-label={t("chat.channelNotes.edit")}
              aria-invalid={editProblem === "too_long" ? "true" : undefined}
              disabled={pending}
              onkeydown={(event) => submitOnShortcut(event, () => saveEdit(note))}
            ></textarea>
            {#if problemMessage(editProblem)}<p class="text-panel-detail text-destructive" role="alert">{problemMessage(editProblem)}</p>{/if}
            <div class="note-edit-actions">
              <button type="button" class="note-text-button" disabled={pending} onclick={() => { editingId = null; }}>{t("chat.channelNotes.cancel")}</button>
              <button type="button" class="note-text-button primary" disabled={pending || editProblem !== null} onclick={() => void saveEdit(note)}>{t("chat.channelNotes.save")}</button>
            </div>
          {:else}
            <div class="note-main">
              <p class="note-body">{note.body}</p>
              <p class="note-meta text-panel-detail text-muted-foreground">
                {#if savedBy}<span>{t("chat.channelNotes.savedBy", savedBy)}</span>{/if}
                {#if suggestedBy}<span>{t("chat.channelNotes.suggestedBy", suggestedBy)}</span>{/if}
                <time datetime={note.updatedAt}>{noteTime(note)}</time>
              </p>
            </div>
            <div class="note-actions">
              <button type="button" class="note-icon-button" aria-label={t("chat.channelNotes.edit")} disabled={pending} onclick={() => startEdit(note)}><Pencil size={ACTION_ICON_SIZE} /></button>
              <button type="button" class="note-icon-button" aria-label={t("chat.channelNotes.delete")} disabled={pending} onclick={() => void deleteNote(note)}><Trash size={ACTION_ICON_SIZE} /></button>
            </div>
          {/if}
        </li>
      {/each}
    </ul>
    {#if error}<Notice class="px-2 pb-1.5 text-panel-detail" message={error} />{/if}
  </div>

  <footer class={cn("relative z-10 grid shrink-0 gap-1.5 bg-popover", mobile ? "p-2" : "p-1.5")}>
    <div class="pointer-events-none absolute inset-x-1.5 top-0 border-t border-border/70" aria-hidden="true"></div>
    {#if addProblem === "limit"}
      <p class="px-1.5 text-panel-detail text-muted-foreground" role="status">{problemMessage(addProblem)}</p>
    {:else}
      <textarea
        class="field min-h-16 resize-none text-[0.8rem]"
        rows="2"
        bind:value={draft}
        placeholder={t("chat.channelNotes.placeholder")}
        aria-label={t("chat.channelNotes.add")}
        aria-invalid={addProblem === "too_long" ? "true" : undefined}
        disabled={pending}
        onkeydown={(event) => submitOnShortcut(event, addNote)}
      ></textarea>
      {#if addProblem === "too_long"}<p class="px-1.5 text-panel-detail text-destructive" role="alert">{problemMessage(addProblem)}</p>{/if}
      <div class="flex justify-end">
        <button type="button" class="note-text-button primary" disabled={pending || addProblem !== null} onclick={() => void addNote()}>{t("chat.channelNotes.add")}</button>
      </div>
    {/if}
  </footer>
{/snippet}

{#if mobileLayout}
  <ProjectPickerMobileDialog label={t("chat.channelNotes.title")} closeLabel={t("chat.channelNotes.close")} {onClose}>
    <div class="surface-floating flex h-full min-h-0 flex-col overflow-hidden rounded-2xl" data-floating-root data-chat-channel-notes-panel>
      {@render content(true)}
    </div>
  </ProjectPickerMobileDialog>
{:else if anchor}
  <div
    class="surface-floating fixed z-80 flex flex-col overflow-hidden outline-none"
    role="dialog"
    aria-label={t("chat.channelNotes.title")}
    tabindex="-1"
    data-floating-root
    data-app-floating-surface
    data-chat-channel-notes-panel
    use:floatPanel={{ anchor, width: FLOATING_WIDTH.lg, horizontalAlign: "end", dismissEnabled: true, onDismiss: onClose }}
  >
    {@render content(false)}
  </div>
{/if}

<style>
  .notes-body.capped { max-height: min(24rem, 60vh); }
  .mobile-header { min-height: 3.5rem; border-bottom: 1px solid color-mix(in srgb, var(--border) 70%, transparent); }
  .notes-close { display: grid; min-width: 3rem; min-height: 3rem; flex: 0 0 auto; place-items: center; border-radius: 0.75rem; }
  .notes-close:active { background: var(--accent); }
  .note-row { display: flex; flex-wrap: wrap; align-items: flex-start; gap: 0.25rem 0.5rem; border-radius: var(--floating-item-radius); padding: 0.4rem 0.5rem; }
  .note-row:hover { background: color-mix(in srgb, var(--accent) 50%, transparent); }
  .note-row > textarea { flex: 1 1 100%; }
  .note-main { display: grid; min-width: 0; flex: 1 1 0; gap: 0.15rem; }
  .note-body { margin: 0; overflow-wrap: anywhere; white-space: pre-wrap; font-size: calc(0.8rem * var(--type-scale)); color: var(--popover-foreground); }
  .note-meta { display: flex; flex-wrap: wrap; gap: 0 0.5rem; margin: 0; }
  .note-actions { display: flex; flex: 0 0 auto; gap: 0.125rem; }
  .note-edit-actions { display: flex; flex: 1 1 100%; justify-content: flex-end; gap: 0.35rem; }
  .note-icon-button { display: grid; width: 1.75rem; height: 1.75rem; place-items: center; border-radius: 0.375rem; color: var(--muted-foreground); }
  .note-icon-button:hover:not(:disabled) { background: var(--accent); color: var(--foreground); }
  .note-text-button { min-height: 1.9rem; border: 1px solid var(--border); border-radius: var(--floating-item-radius); padding: 0.2rem 0.7rem; font-weight: 600; color: var(--foreground); }
  .note-text-button:hover:not(:disabled) { background: var(--accent); }
  .note-text-button.primary { border-color: transparent; background: var(--primary); color: var(--primary-foreground); }
  .note-text-button:disabled,.note-icon-button:disabled { cursor: not-allowed; opacity: 0.5; }
  @media (pointer: coarse) {
    .note-icon-button { width: 2.5rem; height: 2.5rem; }
    .note-text-button { min-height: 2.75rem; }
  }
</style>
