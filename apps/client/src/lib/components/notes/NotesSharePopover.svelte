<script lang="ts">
  import Link2 from "@lucide/svelte/icons/link-2";
  import UserRoundPlus from "@lucide/svelte/icons/user-round-plus";
  import X from "@lucide/svelte/icons/x";
  import { floatPanel } from "$lib/components/contacts/float-panel";
  import LocalPersonAvatar from "$lib/components/contacts/LocalPersonAvatar.svelte";
  import ParticipantPicker from "$lib/components/contacts/ParticipantPicker.svelte";
  import MemberRow from "$lib/components/contacts/MemberRow.svelte";
  import MemberRoleChip from "$lib/components/contacts/MemberRoleChip.svelte";
  import ProjectPickerMobileDialog from "$lib/components/projects/pickers/ProjectPickerMobileDialog.svelte";
  import { FLOATING_WIDTH } from "$lib/components/ui/floating-width";
  import Select from "$lib/components/ui/Select.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getPreferences } from "$lib/stores/preferences.svelte";
  import { cn } from "$lib/utils";

  /**
   * Sharing for one Notes page: who has access, how far the share reaches, and link access. It opens from the page
   * actions and anchors to the Share button on desktop or becomes a sheet on the mobile shell.
   */
  let {
    anchor = null,
    pageTitle,
    mobileLayout = false,
    onClose,
  }: {
    anchor?: HTMLElement | null;
    pageTitle: string;
    mobileLayout?: boolean;
    onClose: () => void;
  } = $props();

  type ShareScope = "page" | "subtree";
  type LinkAccess = "off" | "view" | "comment";

  const AVATAR_SIZE = 26;
  const LINK_ACCESS_DEFAULT: LinkAccess = "off";
  const { t } = getLocalization();
  const preferences = getPreferences();

  let addButton = $state<HTMLButtonElement | null>(null);
  let pickerOpen = $state(false);
  let scope = $state<ShareScope>("page");

  const localName = $derived(preferences.profileDisplayName.trim() || t("contacts.you"));
  const localDetail = $derived(preferences.profileDisplayName.trim() ? t("contacts.you") : null);
  const scopes: readonly { id: ShareScope; label: () => string }[] = [
    { id: "page", label: () => t("notes.shareScopePage") },
    { id: "subtree", label: () => t("notes.shareScopeSubtree") },
  ];
  const linkAccessOptions = $derived([
    { value: "off", label: t("notes.linkAccessOff") },
    { value: "view", label: t("notes.linkAccessView") },
    { value: "comment", label: t("notes.linkAccessComment") },
  ]);
</script>

{#snippet content(mobile: boolean)}
  <header class={cn("flex items-start gap-2", mobile ? "min-h-14 border-b border-border/70 px-4 py-2" : "px-3 pt-3 pb-2")}>
    <div class="min-w-0 flex-1">
      <h2 class="text-[0.933333rem] font-semibold">{t("notes.shareTitle")}</h2>
      <p class="truncate text-panel-detail text-muted-foreground">{pageTitle}</p>
    </div>
    {#if mobile}
      <button type="button" class="share-close" aria-label={t("notes.shareClose")} onclick={onClose}><X size={18} /></button>
    {/if}
  </header>

  <div class={cn("flex min-h-0 flex-1 flex-col gap-2 overflow-y-auto", mobile ? "px-4 py-3" : "px-3 pb-2")}>
    <div class="grid grid-cols-2 gap-1 rounded-md border border-border bg-card p-1 dark:bg-transparent" role="tablist" aria-label={t("notes.shareTitle")}>
      {#each scopes as entry (entry.id)}
        {@const active = scope === entry.id}
        <button
          type="button"
          role="tab"
          aria-selected={active}
          class={cn(
            "flex min-h-7 items-center justify-center rounded-sm px-2 text-center text-[0.8rem] font-medium text-muted-foreground",
            active && "bg-background text-foreground dark:bg-foreground/5",
          )}
          onclick={() => { scope = entry.id; }}
        >
          <span class="truncate">{entry.label()}</span>
        </button>
      {/each}
    </div>

    <div class="menu-label -mx-2">{t("notes.peopleWithAccess")}</div>
    <ul class="-mx-2 grid">
      <MemberRow name={localName} detail={localDetail}>
        {#snippet avatar()}<LocalPersonAvatar size={AVATAR_SIZE} />{/snippet}
        {#snippet trailing()}<MemberRoleChip label={t("notes.shareRoleOwner")} />{/snippet}
      </MemberRow>
    </ul>
    <button bind:this={addButton} type="button" class="menu-item -mx-2" aria-haspopup="dialog" aria-expanded={pickerOpen} onclick={() => { pickerOpen = true; }}>
      <UserRoundPlus class="text-muted-foreground" />
      <span>{t("notes.addPeople")}</span>
    </button>

    <div class="menu-label -mx-2">{t("notes.linkAccess")}</div>
    <div class="flex items-center gap-2">
      <div class="min-w-0 flex-1">
        <Select value={LINK_ACCESS_DEFAULT} options={linkAccessOptions} ariaLabel={t("notes.linkAccess")} inline unavailable class="w-full" onChange={() => {}} />
      </div>
      <button type="button" class="share-link-button control-unavailable" aria-disabled="true"><Link2 size={14} /><span>{t("notes.copyLink")}</span></button>
    </div>
  </div>
{/snippet}

{#if mobileLayout}
  <ProjectPickerMobileDialog label={t("notes.shareTitle")} closeLabel={t("notes.shareClose")} {onClose}>
    <div class="surface-floating flex h-full min-h-0 flex-col overflow-hidden rounded-2xl" data-floating-root data-notes-share-panel>
      {@render content(true)}
    </div>
  </ProjectPickerMobileDialog>
{:else if anchor}
  <div
    class="surface-floating fixed z-80 flex flex-col overflow-hidden outline-none"
    role="dialog"
    aria-label={t("notes.shareTitle")}
    tabindex="-1"
    data-floating-root
    data-app-floating-surface
    data-notes-share-panel
    use:floatPanel={{ anchor, width: FLOATING_WIDTH.lg, horizontalAlign: "end", dismissEnabled: !pickerOpen, onDismiss: onClose }}
  >
    {@render content(false)}
  </div>
{/if}

{#if pickerOpen}
  <ParticipantPicker
    anchor={addButton}
    title={t("contacts.picker.addPeople")}
    includeContacts
    includeTeammates={false}
    space={{ kind: "page", name: pageTitle }}
    onClose={() => { pickerOpen = false; }}
  />
{/if}

<style>
  .share-close { display: grid; min-width: 3rem; min-height: 3rem; flex: 0 0 auto; place-items: center; border-radius: 0.75rem; }
  .share-close:active { background: var(--accent); }
  .share-link-button { display: inline-flex; min-height: 2rem; flex: 0 0 auto; align-items: center; gap: 0.375rem; border: 1px solid var(--border); border-radius: 0.375rem; padding-inline: 0.625rem; font-size: var(--panel-detail-font-size); font-weight: 500; }
  .share-link-button:hover { background: var(--accent); }
</style>
