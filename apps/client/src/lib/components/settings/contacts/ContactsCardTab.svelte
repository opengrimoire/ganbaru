<script lang="ts">
  import ContactCard from "$lib/components/contacts/ContactCard.svelte";
  import Select from "$lib/components/ui/Select.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { isContactTrustDuration, CONTACT_TRUST_DURATIONS } from "$lib/stores/preference-options";
  import { getPreferences } from "$lib/stores/preferences.svelte";

  /** Contact card plus the trust scopes every new contact starts with. Asking every time is the privacy-first default. */
  const { t } = getLocalization();
  const preferences = getPreferences();
  const durationOptions = $derived(CONTACT_TRUST_DURATIONS.map((value) => ({ value, label: t(`contacts.trust.duration.${value}`) })));
</script>

<section class="flex flex-col gap-4">
  <h2 class="px-1 text-[0.866667rem] font-semibold text-foreground">{t("contacts.card.heading")}</h2>
  <div class="px-1 py-1">
    <ContactCard />
  </div>
</section>

<section class="flex flex-col gap-4">
  <h2 class="px-1 text-[0.866667rem] font-semibold text-foreground">{t("contacts.trust.heading")}</h2>
  <div class="flex flex-col gap-3">
    <Select
      label={t("contacts.trust.inviteMe")}
      description={t("contacts.trust.inviteMeDescription")}
      value={preferences.contactsInviteTrustDefault}
      options={durationOptions}
      onChange={(value) => { if (isContactTrustDuration(value)) preferences.setContactsInviteTrustDefault(value); }}
    />
    <Select
      label={t("contacts.trust.messageMe")}
      description={t("contacts.trust.messageMeDescription")}
      value={preferences.contactsMessageTrustDefault}
      options={durationOptions}
      onChange={(value) => { if (isContactTrustDuration(value)) preferences.setContactsMessageTrustDefault(value); }}
    />
  </div>
</section>
