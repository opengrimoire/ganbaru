<script lang="ts">
  import { onMount } from "svelte";
  import Ban from "@lucide/svelte/icons/ban";
  import ContactsEmptyState from "$lib/components/contacts/ContactsEmptyState.svelte";
  import MemberRow from "$lib/components/contacts/MemberRow.svelte";
  import PersonAvatar from "$lib/components/contacts/PersonAvatar.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { contactsMutationErrorKey } from "$lib/contacts/presentation";
  import { getContacts } from "$lib/stores/contacts.svelte";

  /** Contacts the local person blocked. Blocking is silent; unblocking only lets them send a new contact request. */
  const AVATAR_SIZE = 26;
  const { t } = getLocalization();
  const contacts = getContacts();
  const errorMessage = $derived(contacts.error ? t(`contacts.list.error.${contactsMutationErrorKey(contacts.error.code)}`) : null);

  async function unblock(id: string, revision: number): Promise<void> {
    try {
      await contacts.unblock({ id, expectedRevision: revision });
    } catch {
      // Recorded in `contacts.error`.
    }
  }

  onMount(() => {
    void contacts.ensureLoaded();
  });
</script>

<section class="flex flex-col gap-4">
  <h2 class="px-1 text-[0.866667rem] font-semibold text-foreground">{t("contacts.blocked.heading")}</h2>
  {#if errorMessage}
    <Notice class="px-1 text-[0.8rem]" message={errorMessage} onDismiss={() => contacts.clearError()} />
  {/if}
  {#if contacts.blocked.length === 0}
    <div class="px-1">
      <ContactsEmptyState title={t("contacts.blocked.emptyTitle")} description={t("contacts.blocked.emptyDescription")}>
        {#snippet icon()}<Ban size={24} strokeWidth={1.5} />{/snippet}
      </ContactsEmptyState>
    </div>
  {:else}
    <ul class="flex flex-col">
      {#each contacts.blocked as contact (contact.id)}
        <MemberRow name={contact.displayName}>
          {#snippet avatar()}<PersonAvatar displayName={contact.displayName} color={contact.color} size={AVATAR_SIZE} />{/snippet}
          {#snippet trailing()}
            <button type="button" class="contacts-row-action" onclick={() => void unblock(contact.id, contact.revision)}>{t("contacts.blocked.unblock")}</button>
          {/snippet}
        </MemberRow>
      {/each}
    </ul>
  {/if}
</section>

<style>
  .contacts-row-action { display: inline-flex; height: 1.625rem; flex: 0 0 auto; align-items: center; border-radius: 0.375rem; padding-inline: 0.5rem; color: var(--muted-foreground); font-size: calc(0.75rem * var(--type-scale)); font-weight: 500; white-space: nowrap; transition: background-color 120ms ease, color 120ms ease; }
  .contacts-row-action:hover { background: var(--accent); color: var(--foreground); }
</style>
