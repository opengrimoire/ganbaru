<script lang="ts">
  import Inbox from "@lucide/svelte/icons/inbox";
  import Send from "@lucide/svelte/icons/send";
  import InvitePersonDialog from "$lib/components/contacts/InvitePersonDialog.svelte";
  import ContactsEmptyState from "$lib/components/contacts/ContactsEmptyState.svelte";
  import ContactsHeadingAction from "$lib/components/contacts/ContactsHeadingAction.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { cn } from "$lib/utils";

  /** Received and sent invitations. Received ones also surface in the Chat sidebar so they are never missed. */
  type Direction = "received" | "sent";

  const { t } = getLocalization();
  const directions: readonly { id: Direction; label: () => string }[] = [
    { id: "received", label: () => t("contacts.invitations.received") },
    { id: "sent", label: () => t("contacts.invitations.sent") },
  ];
  let direction = $state<Direction>("received");
  let inviteOpen = $state(false);
</script>

<section class="flex flex-col gap-4">
  <div class="flex items-center gap-2 px-1">
    <h2 class="text-[0.866667rem] font-semibold text-foreground">{t("contacts.invitations.heading")}</h2>
    <ContactsHeadingAction label={t("contacts.invitations.invite")} expanded={inviteOpen} onclick={() => { inviteOpen = true; }} />
  </div>
  <div class="flex flex-col gap-3">
    <div class="px-1">
      <div class="inline-grid grid-cols-2 gap-1 rounded-md border border-border bg-card p-1 dark:bg-transparent" role="tablist" aria-label={t("contacts.invitations.heading")}>
        {#each directions as entry (entry.id)}
          {@const active = direction === entry.id}
          <button
            type="button"
            role="tab"
            aria-selected={active}
            class={cn(
              "flex min-h-7 items-center justify-center rounded-sm px-3 text-center text-[0.8rem] font-medium text-muted-foreground",
              active && "bg-background text-foreground dark:bg-foreground/5",
            )}
            onclick={() => { direction = entry.id; }}
          >
            <span>{entry.label()}</span>
          </button>
        {/each}
      </div>
    </div>
    <div class="px-1">
      {#if direction === "received"}
        <ContactsEmptyState title={t("contacts.invitations.emptyReceivedTitle")} description={t("contacts.invitations.emptyReceivedDescription")}>
          {#snippet icon()}<Inbox size={24} strokeWidth={1.5} />{/snippet}
        </ContactsEmptyState>
      {:else}
        <ContactsEmptyState title={t("contacts.invitations.emptySentTitle")} description={t("contacts.invitations.emptySentDescription")}>
          {#snippet icon()}<Send size={24} strokeWidth={1.5} />{/snippet}
        </ContactsEmptyState>
      {/if}
    </div>
  </div>
</section>

{#if inviteOpen}
  <InvitePersonDialog onClose={() => { inviteOpen = false; }} />
{/if}
