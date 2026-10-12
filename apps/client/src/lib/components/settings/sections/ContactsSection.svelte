<script lang="ts">
  import ContactsBlockedTab from "$lib/components/settings/contacts/ContactsBlockedTab.svelte";
  import ContactsCardTab from "$lib/components/settings/contacts/ContactsCardTab.svelte";
  import ContactsListTab from "$lib/components/settings/contacts/ContactsListTab.svelte";
  import ContactsInvitationsTab from "$lib/components/settings/contacts/ContactsInvitationsTab.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import type { ContactsSettingsTab } from "$lib/settings/types";
  import { cn } from "$lib/utils";

  /** Settings > Contacts: the local person's contact card and trust defaults, contacts, invitations, and blocked contacts. */
  let { initialTab = null }: { initialTab?: ContactsSettingsTab | null } = $props();

  const { t } = getLocalization();
  const tabs: readonly { id: ContactsSettingsTab; label: () => string }[] = [
    { id: "card", label: () => t("contacts.tabs.card") },
    { id: "contacts", label: () => t("contacts.tabs.contacts") },
    { id: "invitations", label: () => t("contacts.tabs.invitations") },
    { id: "blocked", label: () => t("contacts.tabs.blocked") },
  ];
  let activeTab = $state<ContactsSettingsTab>("card");

  $effect.pre(() => {
    if (initialTab) activeTab = initialTab;
  });
</script>

<div class="flex flex-col gap-6 pb-4">
  <div
    class="grid grid-cols-2 gap-1 rounded-md border border-border bg-card p-1 min-[560px]:grid-cols-4 dark:bg-transparent"
    role="tablist"
    aria-label={t("contacts.tabs.label")}
  >
    {#each tabs as tab (tab.id)}
      {@const active = activeTab === tab.id}
      <button
        type="button"
        role="tab"
        aria-selected={active}
        class={cn(
          "flex min-h-8 items-center justify-center rounded-sm px-2 text-center text-[0.8rem] font-medium text-muted-foreground",
          active && "bg-background text-foreground dark:bg-foreground/5",
        )}
        onclick={() => { activeTab = tab.id; }}
      >
        <span>{tab.label()}</span>
      </button>
    {/each}
  </div>

  {#if activeTab === "card"}
    <ContactsCardTab />
  {:else if activeTab === "contacts"}
    <ContactsListTab />
  {:else if activeTab === "invitations"}
    <ContactsInvitationsTab />
  {:else}
    <ContactsBlockedTab />
  {/if}
</div>
