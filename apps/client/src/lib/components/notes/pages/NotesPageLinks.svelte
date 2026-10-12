<script lang="ts">
  import Loading from "$lib/components/ui/Loading.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import Select from "$lib/components/ui/Select.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { notesPageTitle } from "$lib/notes/pages/title";
  import type { NotesPage, NotesUnresolvedLink } from "$lib/notes/types";
  import { getNotesEditor } from "$lib/components/notes/editor-context";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import Link2Off from "@lucide/svelte/icons/link-2-off";
  import Plus from "@lucide/svelte/icons/plus";
  import Tags from "@lucide/svelte/icons/tags";
  import X from "@lucide/svelte/icons/x";

  const notes = getNotesEditor();
  const { t } = getLocalization();
  let { embedded = false }: { embedded?: boolean } = $props();
  let aliasesOpen = $state(false);
  let unresolvedOpen = $state(false);
  let aliasDraft = $state("");
  let resolveTargets = $state<Record<string, string>>({});
  const resolveTargetPages = $derived(
    notes.linkResolutionPages.filter((page) => !page.in_trash && !page.archived),
  );

  async function addAlias(): Promise<void> {
    const alias = aliasDraft.trim();
    if (!alias) return;
    await notes.addPageAlias(alias);
    aliasDraft = "";
  }

  /** Loads the aliases again, rejecting while they are still unavailable. */
  async function retryAliases(): Promise<void> {
    await notes.reloadPageAliases();
    if (notes.pageAliasesError) throw new Error(notes.pageAliasesError);
  }

  /** Loads the unresolved links again, rejecting while they are still unavailable. */
  async function retryUnresolvedLinks(): Promise<void> {
    await notes.reloadUnresolvedLinks();
    if (notes.unresolvedLinksError) throw new Error(notes.unresolvedLinksError);
  }

  function selectedTarget(link: NotesUnresolvedLink): string {
    return resolveTargets[link.id] ?? resolveTargetPages[0]?.id ?? "";
  }

  function targetKindLabel(page: NotesPage): string {
    return page.parent.type === "data_source_id"
      ? t("notes.resolveTargetDatabaseRow")
      : t("notes.resolveTargetPage");
  }

  function targetOptionLabel(page: NotesPage): string {
    return t(
      "notes.resolveTargetOption",
      notesPageTitle(page, t("notes.untitled")),
      targetKindLabel(page),
    );
  }

  const aliasesPanelOpen = $derived(embedded || aliasesOpen);
  const unresolvedPanelOpen = $derived(embedded || unresolvedOpen);
</script>

{#if !embedded}
  <div class="mt-2 flex flex-wrap items-center gap-2">
    <button
      type="button"
      class="inline-flex max-w-full items-center gap-1.5 rounded-md px-1.5 py-1 text-[0.733333rem] text-muted-foreground hover:bg-accent hover:text-foreground"
      aria-expanded={aliasesOpen}
      onclick={() => {
        aliasesOpen = !aliasesOpen;
      }}
    >
      <Tags class="size-3.5 shrink-0" />
      <span class="min-w-0 truncate">
        {#if notes.pageAliasesLoading}
          <Loading layout="inline" label={t("notes.loadingPageAliases")} />
        {:else}
          {t("notes.pageAliasesCount", notes.pageAliases.length)}
        {/if}
      </span>
      <ChevronDown class={`size-3.5 shrink-0 transition-transform ${aliasesOpen ? "rotate-180" : ""}`} />
    </button>

    <button
      type="button"
      class="inline-flex max-w-full items-center gap-1.5 rounded-md px-1.5 py-1 text-[0.733333rem] text-muted-foreground hover:bg-accent hover:text-foreground"
      aria-expanded={unresolvedOpen}
      onclick={() => {
        unresolvedOpen = !unresolvedOpen;
      }}
    >
      <Link2Off class="size-3.5 shrink-0" />
      <span class="min-w-0 truncate">
        {#if notes.unresolvedLinksLoading}
          <Loading layout="inline" label={t("notes.loadingUnresolvedLinks")} />
        {:else}
          {t("notes.unresolvedLinksCount", notes.unresolvedLinks.length)}
        {/if}
      </span>
      <ChevronDown class={`size-3.5 shrink-0 transition-transform ${unresolvedOpen ? "rotate-180" : ""}`} />
    </button>
  </div>
{/if}

{#if aliasesPanelOpen}
  <section class={embedded ? "rounded-md bg-background/70 p-2" : "mt-1 max-w-2xl rounded-md border border-border bg-background/70 p-2"}>
    {#if embedded}
      <div class="mb-2 flex min-w-0 items-center gap-1.5 text-[0.8rem] font-medium text-foreground">
        <Tags class="size-3.5 shrink-0 text-muted-foreground" />
        <span class="min-w-0 truncate">
          {#if notes.pageAliasesLoading}
            <Loading layout="inline" label={t("notes.loadingPageAliases")} />
          {:else}
            {t("notes.pageAliasesCount", notes.pageAliases.length)}
          {/if}
        </span>
      </div>
    {/if}
    {#if notes.pageAliasesError}
      <Notice class="text-[0.8rem]" message={t("notes.loadPageAliasesFailed", notes.pageAliasesError)} copy={notes.pageAliasesError} onRetry={retryAliases} />
    {/if}
    <div class="flex min-w-0 flex-wrap items-center gap-2">
      <input
        class="field min-w-36 flex-1 text-[0.8rem] text-foreground"
        bind:value={aliasDraft}
        placeholder={t("notes.pageAliasPlaceholder")}
        aria-label={t("notes.pageAliasInput")}
        onkeydown={(event) => {
          if (event.key === "Enter") {
            event.preventDefault();
            void addAlias();
          }
        }}
      />
      <button
        class="inline-flex items-center gap-1 rounded-md bg-primary px-2 py-1.5 text-[0.8rem] font-medium text-primary-foreground disabled:opacity-50"
        type="button"
        disabled={!aliasDraft.trim()}
        onclick={() => {
          void addAlias();
        }}
      >
        <Plus class="size-3.5" />
        <span>{t("notes.addPageAlias")}</span>
      </button>
    </div>
    <div class="mt-2 flex min-w-0 flex-wrap gap-1.5">
      {#if notes.pageAliases.length === 0}
        <div class="text-[0.8rem] text-muted-foreground">{t("notes.noPageAliases")}</div>
      {:else}
        {#each notes.pageAliases as alias (alias.id)}
          <span class="inline-flex max-w-full items-center gap-1 rounded-md bg-muted px-2 py-1 text-[0.733333rem] text-foreground">
            <span class="truncate">{alias.alias}</span>
            <button
              class="rounded-sm text-muted-foreground hover:text-destructive"
              type="button"
              aria-label={t("notes.deletePageAlias", alias.alias)}
              onclick={() => {
                void notes.deletePageAlias(alias.id);
              }}
            >
              <X class="size-3" />
            </button>
          </span>
        {/each}
      {/if}
    </div>
  </section>
{/if}

{#if unresolvedPanelOpen}
  <section class={embedded ? "mt-2 rounded-md bg-background/70 p-2" : "mt-1 max-w-2xl rounded-md border border-border bg-background/70 p-2"}>
    {#if embedded}
      <div class="mb-2 flex min-w-0 items-center gap-1.5 text-[0.8rem] font-medium text-foreground">
        <Link2Off class="size-3.5 shrink-0 text-muted-foreground" />
        <span class="min-w-0 truncate">
          {#if notes.unresolvedLinksLoading}
            <Loading layout="inline" label={t("notes.loadingUnresolvedLinks")} />
          {:else}
            {t("notes.unresolvedLinksCount", notes.unresolvedLinks.length)}
          {/if}
        </span>
      </div>
    {/if}
    {#if notes.unresolvedLinksError}
      <Notice class="text-[0.8rem]" message={t("notes.loadUnresolvedLinksFailed", notes.unresolvedLinksError)} copy={notes.unresolvedLinksError} onRetry={retryUnresolvedLinks} />
    {:else if notes.unresolvedLinks.length === 0}
      <div class="text-[0.8rem] text-muted-foreground">{t("notes.noUnresolvedLinks")}</div>
    {:else}
      <div class="flex flex-col gap-2">
        {#each notes.unresolvedLinks as link (link.id)}
          <div class="min-w-0 rounded-md bg-muted/40 px-2 py-1.5">
            <div class="truncate text-[0.8rem] font-medium text-foreground">
              {link.raw_target}
            </div>
            <div class="mt-0.5 line-clamp-2 text-[0.733333rem] text-muted-foreground">
              {link.link_text || link.snippet || link.raw_url}
            </div>
            <div class="mt-2 flex min-w-0 flex-wrap items-center gap-2">
              <Select
                inline
                appearance="quiet"
                contentAlign="start"
                class="w-full min-w-0"
                ariaLabel={t("notes.resolveUnresolvedLinkTarget", link.raw_target)}
                value={String(selectedTarget(link) ?? "")}
                options={[...(resolveTargetPages.length === 0 ? [{ value: "", label: t("notes.noResolveTargets") }] : [...(resolveTargetPages).map((page) => ({ value: String(page.id), label: String(targetOptionLabel(page)) }))])]}
                onChange={(nextValue) => {
                  resolveTargets = { ...resolveTargets, [link.id]: nextValue };
                }}
              />
              <button
                class="rounded-md bg-primary px-2 py-1.5 text-[0.8rem] font-medium text-primary-foreground disabled:opacity-50"
                type="button"
                disabled={!selectedTarget(link)}
                onclick={() => {
                  void notes.resolveUnresolvedLink(link.id, selectedTarget(link));
                }}
              >
                {t("notes.resolveUnresolvedLink")}
              </button>
            </div>
          </div>
        {/each}
      </div>
    {/if}
  </section>
{/if}
