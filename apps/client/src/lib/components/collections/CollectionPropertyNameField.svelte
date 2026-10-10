<script lang="ts">
  import { untrack } from "svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import CollectionPropertyIconButton from "./CollectionPropertyIconButton.svelte";
  import type { CollectionPropertyKind } from "./property-icons";

  /**
   * Shows a property's icon button and name at the top of its column menu, renaming it on Enter, blur, or when the menu closes.
   * A pristine draft follows canonical renames; an active or rejected draft is kept until the property itself changes.
   * Properties that are not editable keep both their name and icon.
   */
  let { propertyId, name, kind, icon = null, editable = true, maxLength, onRename, onIconChange }: {
    propertyId: string;
    name: string;
    kind: CollectionPropertyKind;
    /** The chosen `lucide:` icon; null shows the type icon. */
    icon?: string | null;
    editable?: boolean;
    maxLength?: number;
    /** Persist the trimmed, changed name; a rejection keeps the draft and shows its message. */
    onRename: (name: string) => Promise<void>;
    /** Persist a chosen icon, or null to return to the type icon. */
    onIconChange?: (icon: string | null) => Promise<void>;
  } = $props();

  const { t } = getLocalization();
  let draft = $state(untrack(() => name));
  let canonical = $state(untrack(() => name));
  let draftPropertyId = $state(untrack(() => propertyId));
  let editing = $state(false);
  let saving = $state(false);
  let error = $state<string | null>(null);
  let iconError = $state<string | null>(null);
  let generation = 0;

  $effect(() => {
    const currentId = propertyId;
    const currentName = name;
    const active = editing || saving || error !== null;
    untrack(() => {
      if (currentId !== draftPropertyId) {
        generation += 1;
        draftPropertyId = currentId;
        canonical = currentName;
        draft = currentName;
        editing = false;
        saving = false;
        error = null;
        iconError = null;
      } else if (!active && draft === canonical) {
        draft = currentName;
        canonical = currentName;
      }
    });
  });

  // Menus can close without blurring the field first; a draft whose save was already rejected is not retried.
  $effect(() => () => untrack(() => { if (!saving && error === null) void commit(); }));

  /** Save a changed name, restoring the canonical name when the draft is blank or unchanged. */
  async function commit(): Promise<void> {
    if (!editable || saving) return;
    const next = draft.trim();
    if (!next || next === canonical) {
      draft = canonical;
      error = null;
      return;
    }
    const current = ++generation;
    saving = true;
    error = null;
    try {
      await onRename(next);
      if (current === generation) {
        draft = next;
        canonical = next;
      }
    } catch (caught: unknown) {
      if (current === generation) error = t("collections.property.renameFailed", caught instanceof Error ? caught.message : String(caught));
    } finally {
      if (current === generation) saving = false;
    }
  }

  /** Save a chosen icon, showing a rejection in the field's message area. */
  async function changeIcon(next: string | null): Promise<void> {
    if (!onIconChange || next === icon) return;
    iconError = null;
    try {
      await onIconChange(next);
    } catch (caught: unknown) {
      iconError = t("collections.property.iconFailed", caught instanceof Error ? caught.message : String(caught));
    }
  }
</script>

<div class="grid gap-1 pb-1.5">
  <div class="flex min-w-0 items-center gap-1.5">
    <CollectionPropertyIconButton class="collection-menu-control" {icon} {kind} editable={editable && onIconChange !== undefined} onChange={(next) => void changeIcon(next)} />
    <input class={["h-(--panel-row-height) min-w-0 flex-1 rounded-md border border-border bg-transparent px-2 text-[length:inherit] outline-none read-only:text-muted-foreground", !editable && "cursor-not-allowed"]} aria-label={t("collections.property.name")}
      data-collection-menu-initial-focus readonly={!editable || saving} maxlength={maxLength} bind:value={draft}
      onfocus={() => { editing = true; }}
      onblur={() => { editing = false; void commit(); }}
      onkeydown={(event) => {
        event.stopPropagation();
        if (event.key !== "Enter" || event.isComposing) return;
        event.preventDefault();
        void commit();
      }} />
  </div>
  {#each [error, iconError].filter(Boolean) as message}<p role="alert" class="px-1 text-destructive">{message}</p>{/each}
</div>
