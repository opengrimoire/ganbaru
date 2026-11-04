<script lang="ts">
  import { untrack } from "svelte";
  import { normalizeContactsError, type Contact } from "$lib/api/contacts";
  import Select from "$lib/components/ui/Select.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { isolateText } from "$lib/i18n/formatters";
  import { contactsMutationErrorKey, type ContactsMutationErrorKey } from "$lib/contacts/presentation";
  import { trustDurationForKind, trustKindForDuration } from "$lib/contacts/trust";
  import { isContactTrustDuration, CONTACT_TRUST_DURATIONS, type ContactTrustDuration } from "$lib/stores/preference-options";
  import { getContacts } from "$lib/stores/contacts.svelte";
  import ContactsDialog from "./ContactsDialog.svelte";

  /** Edits what one contact may do without asking. Timed grants restart from the moment they are saved. */
  let {
    contact,
    onClose,
  }: {
    contact: Contact;
    onClose: () => void;
  } = $props();

  const { t } = getLocalization();
  const contacts = getContacts();
  let invite = $state<ContactTrustDuration>(untrack(() => trustDurationForKind(contact.inviteTrust)));
  let message = $state<ContactTrustDuration>(untrack(() => trustDurationForKind(contact.messageTrust)));
  let saving = $state(false);
  let error = $state<ContactsMutationErrorKey | null>(null);
  const durationOptions = $derived(CONTACT_TRUST_DURATIONS.map((value) => ({ value, label: t(`contacts.trust.duration.${value}`) })));

  async function save(): Promise<void> {
    if (saving) return;
    saving = true;
    error = null;
    try {
      await contacts.updateTrust(
        { id: contact.id, expectedRevision: contact.revision },
        { inviteTrust: trustKindForDuration(invite), messageTrust: trustKindForDuration(message) },
      );
      onClose();
    } catch (cause: unknown) {
      error = contactsMutationErrorKey(normalizeContactsError(cause).code);
    } finally {
      saving = false;
    }
  }
</script>

<ContactsDialog
  title={t("contacts.list.editTrust")}
  description={t("contacts.list.editTrustDescription", isolateText(contact.displayName))}
  cancelLabel={t("contacts.invite.cancel")}
  confirmLabel={t("contacts.list.save")}
  confirmDisabled={saving}
  onConfirm={() => { void save(); }}
  {onClose}
>
  <Select
    label={t("contacts.trust.inviteMe")}
    description={t("contacts.trust.inviteMeDescription")}
    value={invite}
    options={durationOptions}
    onChange={(value) => { if (isContactTrustDuration(value)) invite = value; }}
  />
  <Select
    label={t("contacts.trust.messageMe")}
    description={t("contacts.trust.messageMeDescription")}
    value={message}
    options={durationOptions}
    onChange={(value) => { if (isContactTrustDuration(value)) message = value; }}
  />
  {#if error}
    <Notice class="text-panel-detail" message={t(`contacts.list.error.${error}`)} />
  {/if}
</ContactsDialog>
