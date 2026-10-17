<script lang="ts">
  import { onMount } from "svelte";
  import ContactRound from "@lucide/svelte/icons/contact-round";
  import Ellipsis from "@lucide/svelte/icons/ellipsis";
  import Search from "@lucide/svelte/icons/search";
  import type { Contact, ContactRequest } from "$lib/api/contacts";
  import AddContactDialog from "$lib/components/contacts/AddContactDialog.svelte";
  import ContactTrustDialog from "$lib/components/contacts/ContactTrustDialog.svelte";
  import ContactsEmptyState from "$lib/components/contacts/ContactsEmptyState.svelte";
  import ContactsHeadingAction from "$lib/components/contacts/ContactsHeadingAction.svelte";
  import MemberRow from "$lib/components/contacts/MemberRow.svelte";
  import PersonAvatar from "$lib/components/contacts/PersonAvatar.svelte";
  import ConfirmDialog from "$lib/components/ui/ConfirmDialog.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { formatRelativeDays } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { formatContactTrustDetail, contactsMutationErrorKey } from "$lib/contacts/presentation";
  import { daysUntil, trustKindForDuration } from "$lib/contacts/trust";
  import { getContacts } from "$lib/stores/contacts.svelte";
  import { getPreferences } from "$lib/stores/preferences.svelte";

  /**
   * Contacts and the requests on their way in and out. Received requests are accepted with the trust defaults from
   * the card tab; everything else a contact may do is edited per row. The list fills only through mutual requests.
   */
  const AVATAR_SIZE = 26;
  const { t } = getLocalization();
  const contacts = getContacts();
  const preferences = getPreferences();
  let query = $state("");
  let addContactOpen = $state(false);
  let notice = $state<string | null>(null);
  let menuFor = $state<string | null>(null);
  let trustTarget = $state<Contact | null>(null);
  let removeTarget = $state<Contact | null>(null);
  let blockTarget = $state<{ publicKey: string; displayName: string } | null>(null);
  let now = $state(new Date());

  const filtered = $derived(contacts.filter(query));
  const errorMessage = $derived(contacts.error ? t(`contacts.list.error.${contactsMutationErrorKey(contacts.error.code)}`) : null);

  function trustDefaults() {
    return {
      inviteTrust: trustKindForDuration(preferences.contactsInviteTrustDefault),
      messageTrust: trustKindForDuration(preferences.contactsMessageTrustDefault),
    };
  }

  function row(entry: { id: string; revision: number }) {
    return { id: entry.id, expectedRevision: entry.revision };
  }

  /** Runs one store mutation; the store records the failure and the tab shows it under the heading. */
  async function run(action: () => Promise<void>): Promise<void> {
    try {
      await action();
    } catch {
      // Recorded in `contacts.error`.
    }
  }

  function requestDetail(request: ContactRequest): string {
    const expiry = t("contacts.list.expiresIn", formatRelativeDays(t, daysUntil(request.expiresAt, now)));
    const status = request.lastErrorCode ? `${expiry}. ${t("contacts.list.deliveryRetrying")}` : expiry;
    return `${request.verificationCode} · ${status}`;
  }

  function toggleMenu(contactId: string): void {
    menuFor = menuFor === contactId ? null : contactId;
  }

  onMount(() => {
    void contacts.ensureLoaded().then(() => contacts.syncRequests());
    const clock = window.setInterval(() => { now = new Date(); }, 60_000);
    const onPointerDown = (event: PointerEvent) => {
      if (menuFor && event.target instanceof Element && !event.target.closest("[data-contact-menu]")) menuFor = null;
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape" && menuFor) {
        event.stopPropagation();
        menuFor = null;
      }
    };
    window.addEventListener("pointerdown", onPointerDown);
    window.addEventListener("keydown", onKeyDown, true);
    return () => {
      window.clearInterval(clock);
      window.removeEventListener("pointerdown", onPointerDown);
      window.removeEventListener("keydown", onKeyDown, true);
    };
  });
</script>

{#if contacts.receivedPending.length > 0}
  <section class="flex flex-col gap-3">
    <h2 class="px-1 text-[0.866667rem] font-semibold text-foreground">{t("contacts.list.requestsReceived")}</h2>
    <ul class="flex flex-col">
      {#each contacts.receivedPending as request (request.id)}
        <MemberRow name={request.displayName} detail={request.verificationCode}>
          {#snippet avatar()}<PersonAvatar displayName={request.displayName} color={request.color} size={AVATAR_SIZE} />{/snippet}
          {#snippet trailing()}
            <button type="button" class="contacts-row-action contacts-row-action-primary" onclick={() => void run(() => contacts.accept(row(request), trustDefaults()))}>{t("contacts.invitations.accept")}</button>
            <button type="button" class="contacts-row-action" onclick={() => void run(() => contacts.decline(row(request)))}>{t("contacts.invitations.decline")}</button>
            <button type="button" class="contacts-row-action" onclick={() => { blockTarget = { publicKey: request.publicKey, displayName: request.displayName }; }}>{t("contacts.invitations.block")}</button>
          {/snippet}
        </MemberRow>
      {/each}
    </ul>
  </section>
{/if}

{#if contacts.sentPending.length > 0}
  <section class="flex flex-col gap-3">
    <h2 class="px-1 text-[0.866667rem] font-semibold text-foreground">{t("contacts.list.requestsSent")}</h2>
    <ul class="flex flex-col">
      {#each contacts.sentPending as request (request.id)}
        <MemberRow name={request.displayName} detail={requestDetail(request)}>
          {#snippet avatar()}<PersonAvatar displayName={request.displayName} color={request.color} size={AVATAR_SIZE} />{/snippet}
          {#snippet trailing()}
            <button type="button" class="contacts-row-action" onclick={() => void run(() => contacts.cancel(row(request)))}>{t("contacts.list.cancel")}</button>
          {/snippet}
        </MemberRow>
      {/each}
    </ul>
  </section>
{/if}

<section class="flex flex-col gap-4">
  <div class="flex items-center gap-2 px-1">
    <h2 class="text-[0.866667rem] font-semibold text-foreground">{t("contacts.list.heading")}</h2>
    <ContactsHeadingAction label={t("contacts.list.add")} expanded={addContactOpen} onclick={() => { addContactOpen = true; }} />
  </div>
  {#if errorMessage}
    <Notice class="px-1 text-[0.8rem]" message={errorMessage} onDismiss={() => { notice = null; contacts.clearError(); }} />
  {:else if notice}
    <div class="flex items-start gap-2 px-1 text-[0.8rem]" aria-live="polite">
      <span class="flex-1 text-muted-foreground">{notice}</span>
      <button type="button" class="contacts-row-action" onclick={() => { notice = null; contacts.clearError(); }}>{t("contacts.list.dismiss")}</button>
    </div>
  {/if}
  <div class="flex flex-col gap-3">
    <label class="contacts-search field mx-1 flex items-center gap-1.5 text-muted-foreground">
      <Search size={13} aria-hidden="true" />
      <input class="field-bare text-foreground" type="search" bind:value={query} aria-label={t("contacts.list.search")} placeholder={t("contacts.list.search")} />
    </label>
    {#if contacts.activeContacts.length === 0}
      <div class="px-1">
        <ContactsEmptyState title={t("contacts.list.emptyTitle")} description={t("contacts.list.emptyDescription")}>
          {#snippet icon()}<ContactRound size={24} strokeWidth={1.5} />{/snippet}
        </ContactsEmptyState>
      </div>
    {:else if filtered.length === 0}
      <p class="px-2 text-[0.8rem] text-muted-foreground">{t("contacts.list.noMatches")}</p>
    {:else}
      <ul class="flex flex-col">
        {#each filtered as contact (contact.id)}
          <MemberRow name={contact.displayName} detail={formatContactTrustDetail(t, contact, now)}>
            {#snippet avatar()}<PersonAvatar displayName={contact.displayName} color={contact.color} size={AVATAR_SIZE} />{/snippet}
            {#snippet trailing()}
              <span class="relative" data-contact-menu>
                <button type="button" class="contacts-row-icon" aria-label={t("contacts.list.actions", contact.displayName)} aria-haspopup="menu" aria-expanded={menuFor === contact.id} onclick={() => toggleMenu(contact.id)}><Ellipsis size={16} /></button>
                {#if menuFor === contact.id}
                  <div class="surface-floating surface-floating-body absolute right-0 top-full z-20 mt-1 flex min-w-36 flex-col" role="menu">
                    <button type="button" role="menuitem" class="menu-item" onclick={() => { menuFor = null; trustTarget = contact; }}>{t("contacts.list.editTrust")}</button>
                    <button type="button" role="menuitem" class="menu-item" onclick={() => { menuFor = null; removeTarget = contact; }}>{t("contacts.list.remove")}</button>
                    <button type="button" role="menuitem" class="menu-item text-destructive" onclick={() => { menuFor = null; blockTarget = contact; }}>{t("contacts.list.block")}</button>
                  </div>
                {/if}
              </span>
            {/snippet}
          </MemberRow>
        {/each}
      </ul>
    {/if}
  </div>
</section>

{#if addContactOpen}
  <AddContactDialog onClose={() => { addContactOpen = false; }} onSent={() => { notice = t("contacts.addContact.sent"); }} />
{/if}

{#if trustTarget}
  <ContactTrustDialog contact={trustTarget} onClose={() => { trustTarget = null; }} />
{/if}

{#if removeTarget}
  {@const target = removeTarget}
  <ConfirmDialog
    title={t("contacts.list.removeTitle")}
    message={t("contacts.list.removeMessage", target.displayName)}
    confirmLabel={t("contacts.list.remove")}
    cancelLabel={t("contacts.invite.cancel")}
    onConfirm={() => { removeTarget = null; void run(() => contacts.remove(row(target))); }}
    onCancel={() => { removeTarget = null; }}
  />
{/if}

{#if blockTarget}
  {@const target = blockTarget}
  <ConfirmDialog
    title={t("contacts.list.blockTitle")}
    message={t("contacts.list.blockMessage", target.displayName)}
    confirmLabel={t("contacts.list.block")}
    cancelLabel={t("contacts.invite.cancel")}
    onConfirm={() => { blockTarget = null; void run(() => contacts.block(target.publicKey)); }}
    onCancel={() => { blockTarget = null; }}
  />
{/if}

<style>
  .contacts-search { min-height: 2rem; padding-block: 0; font-size: calc(0.8rem * var(--type-scale)); }
  .contacts-search input::-webkit-search-cancel-button { display: none; }
  .contacts-row-action { display: inline-flex; height: 1.625rem; flex: 0 0 auto; align-items: center; border-radius: 0.375rem; padding-inline: 0.5rem; color: var(--muted-foreground); font-size: calc(0.75rem * var(--type-scale)); font-weight: 500; white-space: nowrap; transition: background-color 120ms ease, color 120ms ease; }
  .contacts-row-action:hover { background: var(--accent); color: var(--foreground); }
  .contacts-row-action-primary { color: var(--foreground); }
  .contacts-row-icon { display: grid; width: 1.75rem; height: 1.75rem; flex: 0 0 auto; place-items: center; border-radius: 0.375rem; color: var(--muted-foreground); }
  .contacts-row-icon:hover, .contacts-row-icon[aria-expanded="true"] { background: var(--accent); color: var(--foreground); }
</style>
