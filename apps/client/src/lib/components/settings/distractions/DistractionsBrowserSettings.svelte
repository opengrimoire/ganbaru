<script lang="ts">
  import Pencil from "@lucide/svelte/icons/pencil";
  import Plus from "@lucide/svelte/icons/plus";
  import Save from "@lucide/svelte/icons/save";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import { cn } from "$lib/utils";
  import {
    DISTRACTIONS_CATEGORY_DEFINITIONS,
    getDistractionsCategoryDefinition,
    parseDistractionsHosts,
    type DistractionsCategoryId,
    type DistractionsCustomCategory,
    type DistractionsMode,
    type DistractionsHostRule,
  } from "$lib/distractions";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getDistractions } from "$lib/stores/distractions.svelte";
  import ConfirmDialog from "$lib/components/ui/ConfirmDialog.svelte";
  import DistractionsBrowserConnectionStatus from "$lib/components/settings/distractions/DistractionsBrowserConnectionStatus.svelte";
  import DistractionsConfigurationSection from "./DistractionsConfigurationSection.svelte";
  import DistractionsRuleList from "./DistractionsRuleList.svelte";

  const distractions = getDistractions();
  const { t } = getLocalization();
  let { showConnectionStatus = true }: { showConnectionStatus?: boolean } = $props();

  type WebsiteListKind = "blocked" | "exception" | "allowed";

  interface WebsiteListSection {
    kind: WebsiteListKind;
    id: string;
    heading: string;
    description: string;
    placeholder: string;
    emptyText: string;
    errorText: string;
    websites: () => readonly DistractionsHostRule[];
    add: (text: string) => boolean;
    remove: (website: string) => void;
    setEnabled: (website: string, enabled: boolean) => void;
  }

  interface RuleListItem {
    id: string;
    label: string;
    enabled: boolean;
  }

  interface PendingWebsiteAction {
    type: "disable" | "delete";
    kind: WebsiteListKind;
    host: string;
  }

  type BrowserConfigurationToggle = "enabled" | "focus" | "shortBreaks" | "longBreaks" | "pause";

  interface PendingBrowserConfigurationAction {
    toggle: BrowserConfigurationToggle;
  }

  interface PendingModeAction {
    mode: DistractionsMode;
  }

  interface PendingCategoryAction {
    type: "disable";
    categoryId: DistractionsCategoryId;
  }

  interface PendingCustomCategoryAction {
    type: "disable" | "delete";
    customCategoryId: string;
  }

  interface PendingCustomCategoryDraftHostAction {
    host: string;
  }

  type PendingAction =
    | { target: "website"; action: PendingWebsiteAction }
    | { target: "browserConfiguration"; action: PendingBrowserConfigurationAction }
    | { target: "mode"; action: PendingModeAction }
    | { target: "category"; action: PendingCategoryAction }
    | { target: "customCategory"; action: PendingCustomCategoryAction }
    | { target: "customCategoryDraftHost"; action: PendingCustomCategoryDraftHostAction };

  type CustomCategoryDraftField = "name" | "hosts";

  interface CustomCategoryDraft {
    name: string;
    hostInput: string;
    hosts: string[];
  }

  const websiteSections = {
    blocked: {
      kind: "blocked",
      id: "distractions-blocked-websites",
      heading: t("settings.distractions.browser.blockedWebsites"),
      description: t("settings.distractions.browser.blockedWebsitesDescription"),
      placeholder: t("settings.distractions.browser.enterDomain"),
      emptyText: t("settings.distractions.browser.noBlockedWebsites"),
      errorText: t("settings.distractions.browser.invalidDomain"),
      websites: () => distractions.blockedHosts,
      add: (text: string) => distractions.addBlockedHostsText(text),
      remove: (website: string) => distractions.removeBlockedHost(website),
      setEnabled: (website: string, enabled: boolean) => distractions.setBlockedHostEnabled(website, enabled),
    },
    exception: {
      kind: "exception",
      id: "distractions-exception-websites",
      heading: t("settings.distractions.browser.exceptions"),
      description: t("settings.distractions.browser.exceptionsDescription"),
      placeholder: t("settings.distractions.browser.enterDomain"),
      emptyText: t("settings.distractions.browser.noExceptions"),
      errorText: t("settings.distractions.browser.invalidDomain"),
      websites: () => distractions.exceptionHosts,
      add: (text: string) => distractions.addExceptionHostsText(text),
      remove: (website: string) => distractions.removeExceptionHost(website),
      setEnabled: (website: string, enabled: boolean) => distractions.setExceptionHostEnabled(website, enabled),
    },
    allowed: {
      kind: "allowed",
      id: "distractions-allowed-websites",
      heading: t("settings.distractions.browser.allowedWebsites"),
      description: t("settings.distractions.browser.allowedWebsitesDescription"),
      placeholder: t("settings.distractions.browser.enterDomain"),
      emptyText: t("settings.distractions.browser.noAllowedWebsites"),
      errorText: t("settings.distractions.browser.invalidDomain"),
      websites: () => distractions.allowedHosts,
      add: (text: string) => distractions.addAllowedHostsText(text),
      remove: (website: string) => distractions.removeAllowedHost(website),
      setEnabled: (website: string, enabled: boolean) => distractions.setAllowedHostEnabled(website, enabled),
    },
  } satisfies Record<WebsiteListKind, WebsiteListSection>;
  const blacklistWebsiteSections: readonly WebsiteListSection[] = [
    websiteSections.blocked,
    websiteSections.exception,
  ];
  const whitelistWebsiteSections: readonly WebsiteListSection[] = [
    websiteSections.allowed,
  ];

  let customCategoryDraft = $state<CustomCategoryDraft>({
    name: "",
    hostInput: "",
    hosts: [],
  });
  let customCategoryErrors = $state<Record<CustomCategoryDraftField, string>>({
    name: "",
    hosts: "",
  });
  let pendingAction = $state<PendingAction | null>(null);
  let customCategoryFormOpen = $state(false);
  let customCategoryEditingId = $state<string | null>(null);

  function websiteItems(section: WebsiteListSection): RuleListItem[] {
    return section.websites().map((rule) => ({
      id: rule.host,
      label: rule.host,
      enabled: rule.enabled,
    }));
  }

  function requestHostEnabledChange(section: WebsiteListSection, host: string, enabled: boolean): void {
    if (enabled) {
      section.setEnabled(host, true);
      return;
    }
    pendingAction = {
      target: "website",
      action: {
        type: "disable",
        kind: section.kind,
        host,
      },
    };
  }

  function requestHostDelete(section: WebsiteListSection, host: string): void {
    pendingAction = {
      target: "website",
      action: {
        type: "delete",
        kind: section.kind,
        host,
      },
    };
  }

  function categoryEnabled(categoryId: DistractionsCategoryId): boolean {
    return distractions.blockedCategories.find((rule) => rule.id === categoryId)?.enabled ?? false;
  }

  function requestCategoryEnabledChange(categoryId: DistractionsCategoryId, enabled: boolean): void {
    if (enabled) {
      distractions.setBlockedCategoryEnabled(categoryId, true);
      return;
    }
    pendingAction = {
      target: "category",
      action: {
        type: "disable",
        categoryId,
      },
    };
  }

  function requestCustomCategoryEnabledChange(customCategory: DistractionsCustomCategory, enabled: boolean): void {
    if (enabled) {
      distractions.setCustomCategoryEnabled(customCategory.id, true);
      return;
    }
    pendingAction = {
      target: "customCategory",
      action: {
        type: "disable",
        customCategoryId: customCategory.id,
      },
    };
  }

  function requestCustomCategoryDelete(customCategory: DistractionsCustomCategory): void {
    pendingAction = {
      target: "customCategory",
      action: {
        type: "delete",
        customCategoryId: customCategory.id,
      },
    };
  }

  function requestEditingCustomCategoryDelete(): void {
    if (!customCategoryEditingId) return;
    const customCategory = findCustomCategory(customCategoryEditingId);
    if (!customCategory) return;
    requestCustomCategoryDelete(customCategory);
  }

  function findCustomCategory(customCategoryId: string): DistractionsCustomCategory | null {
    return distractions.customCategories.find((customCategory) => customCategory.id === customCategoryId) ?? null;
  }

  function clearCustomCategoryError(field: CustomCategoryDraftField): void {
    customCategoryErrors[field] = "";
  }

  function clearCustomCategoryForm(): void {
    customCategoryDraft.name = "";
    customCategoryDraft.hostInput = "";
    customCategoryDraft.hosts = [];
    customCategoryErrors.name = "";
    customCategoryErrors.hosts = "";
    customCategoryEditingId = null;
  }

  function closeCustomCategoryForm(): void {
    clearCustomCategoryForm();
    customCategoryFormOpen = false;
  }

  function openNewCustomCategoryForm(): void {
    clearCustomCategoryForm();
    customCategoryFormOpen = true;
  }

  function openEditCustomCategoryForm(customCategory: DistractionsCustomCategory): void {
    customCategoryDraft.name = customCategory.name;
    customCategoryDraft.hostInput = "";
    customCategoryDraft.hosts = customCategory.hosts.map((rule) => rule.host);
    customCategoryErrors.name = "";
    customCategoryErrors.hosts = "";
    customCategoryEditingId = customCategory.id;
    customCategoryFormOpen = true;
  }

  function addCustomCategoryDraftHost(): boolean {
    const hosts = parseDistractionsHosts(customCategoryDraft.hostInput);
    if (hosts.length === 0) {
      customCategoryErrors.hosts = t("settings.distractions.browser.invalidDomain");
      return false;
    }
    const newHosts = hosts.filter((host) => !customCategoryDraft.hosts.includes(host));
    if (newHosts.length === 0) {
      customCategoryErrors.hosts = t("settings.distractions.browser.duplicateDomain");
      customCategoryDraft.hostInput = "";
      return false;
    }
    customCategoryDraft.hosts = [...customCategoryDraft.hosts, ...newHosts];
    customCategoryDraft.hostInput = "";
    customCategoryErrors.hosts = "";
    return true;
  }

  function submitCustomCategoryDraftHost(event: SubmitEvent): void {
    event.preventDefault();
    addCustomCategoryDraftHost();
  }

  function handleCustomCategoryNameKeydown(event: KeyboardEvent): void {
    if (event.key !== "Enter") return;
    event.preventDefault();
    if (event.ctrlKey || event.metaKey) saveCustomCategoryWithPendingHost();
  }

  function saveCustomCategoryWithPendingHost(): void {
    if (customCategoryDraft.hostInput.trim() && !addCustomCategoryDraftHost()) return;
    saveCustomCategory();
  }

  function removeCustomCategoryDraftHost(host: string): void {
    customCategoryDraft.hosts = customCategoryDraft.hosts.filter((draftHost) => draftHost !== host);
    customCategoryErrors.hosts = "";
  }

  function requestCustomCategoryDraftHostDelete(host: string): void {
    pendingAction = {
      target: "customCategoryDraftHost",
      action: { host },
    };
  }

  function saveCustomCategory(): void {
    customCategoryErrors.name = "";
    customCategoryErrors.hosts = "";
    const hostsText = customCategoryDraft.hosts.join(" ");
    const result = customCategoryEditingId
      ? distractions.updateCustomCategory(
        customCategoryEditingId,
        customCategoryDraft.name,
        hostsText,
      )
      : distractions.addCustomCategory(customCategoryDraft.name, hostsText);
    if (result === "added" || result === "updated") {
      clearCustomCategoryForm();
      customCategoryFormOpen = false;
      return;
    }
    if (result === "invalid-name") {
      customCategoryErrors.name = t("settings.distractions.browser.categoryNameRequired");
    } else if (result === "duplicate-name") {
      customCategoryErrors.name = t("settings.distractions.browser.differentCategoryName");
    } else if (result === "missing") {
      customCategoryErrors.name = t("settings.distractions.browser.categoryMissing");
    } else {
      customCategoryErrors.hosts = t("settings.distractions.browser.invalidDomain");
    }
  }

  function setBrowserConfigurationToggle(
    toggle: BrowserConfigurationToggle,
    checked: boolean,
  ): void {
    if (toggle === "enabled") {
      distractions.setEnabled(checked);
    } else if (toggle === "focus") {
      distractions.setBlockDuringFocus(checked);
    } else if (toggle === "shortBreaks") {
      distractions.setBlockDuringShortBreaks(checked);
    } else if (toggle === "longBreaks") {
      distractions.setBlockDuringLongBreaks(checked);
    } else {
      distractions.setPauseDuringFocusPause(checked);
    }
  }

  function requestBrowserConfigurationToggleChange(
    toggle: BrowserConfigurationToggle,
    checked: boolean,
  ): void {
    if (checked) {
      setBrowserConfigurationToggle(toggle, true);
      return;
    }
    pendingAction = { target: "browserConfiguration", action: { toggle } };
  }

  function requestModeChange(mode: DistractionsMode): void {
    if (mode === distractions.mode) return;
    if (mode === "whitelist") {
      pendingAction = { target: "mode", action: { mode } };
      return;
    }
    distractions.setMode(mode);
  }

  function categoryLabel(categoryId: DistractionsCategoryId): string {
    switch (categoryId) {
      case "social-media":
        return t("settings.distractions.browser.categoryLabel.socialMedia");
      case "streaming":
        return t("settings.distractions.browser.categoryLabel.streaming");
      case "news":
        return t("settings.distractions.browser.categoryLabel.news");
      case "sports":
        return t("settings.distractions.browser.categoryLabel.sports");
      case "porn":
        return t("settings.distractions.browser.categoryLabel.porn");
      case "gambling":
        return t("settings.distractions.browser.categoryLabel.gambling");
      case "gaming":
        return t("settings.distractions.browser.categoryLabel.gaming");
      case "shopping":
        return t("settings.distractions.browser.categoryLabel.shopping");
      case "dating":
        return t("settings.distractions.browser.categoryLabel.dating");
      case "trading":
        return t("settings.distractions.browser.categoryLabel.trading");
    }
  }

  function confirmPendingAction(): void {
    if (!pendingAction) return;
    if (pendingAction.target === "website") {
      const { type, kind, host } = pendingAction.action;
      const section = websiteSections[kind];
      if (type === "disable") {
        section.setEnabled(host, false);
      } else {
        section.remove(host);
      }
    } else if (pendingAction.target === "browserConfiguration") {
      setBrowserConfigurationToggle(pendingAction.action.toggle, false);
    } else if (pendingAction.target === "mode") {
      distractions.setMode(pendingAction.action.mode);
    } else if (pendingAction.target === "category") {
      distractions.setBlockedCategoryEnabled(pendingAction.action.categoryId, false);
    } else if (pendingAction.target === "customCategoryDraftHost") {
      removeCustomCategoryDraftHost(pendingAction.action.host);
    } else {
      const { type, customCategoryId } = pendingAction.action;
      if (type === "disable") {
        distractions.setCustomCategoryEnabled(customCategoryId, false);
      } else {
        distractions.removeCustomCategory(customCategoryId);
        if (customCategoryEditingId === customCategoryId) {
          closeCustomCategoryForm();
        }
      }
    }
    pendingAction = null;
  }

  function cancelPendingAction(): void {
    pendingAction = null;
  }

  function pendingActionTitle(action: PendingAction): string {
    if (action.target === "website") {
      return action.action.type === "disable"
        ? t("settings.distractions.browser.websiteDisableTitle", action.action.host)
        : t("settings.distractions.browser.websiteDeleteTitle", action.action.host);
    }
    if (action.target === "browserConfiguration") {
      if (action.action.toggle === "enabled") return t("settings.distractions.browser.turnOffBrowserTitle");
      if (action.action.toggle === "focus") return t("settings.distractions.browser.allowWebsitesFocusTitle");
      if (action.action.toggle === "shortBreaks") return t("settings.distractions.browser.allowWebsitesShortBreaksTitle");
      if (action.action.toggle === "longBreaks") return t("settings.distractions.browser.allowWebsitesLongBreaksTitle");
      return t("settings.distractions.browser.keepBrowserBlockingPausedTitle");
    }
    if (action.target === "mode") return t("settings.distractions.browser.switchWhitelistTitle");
    if (action.target === "category") {
      const category = getDistractionsCategoryDefinition(action.action.categoryId);
      return t(
        "settings.distractions.browser.allowCategoryTitle",
        category ? categoryLabel(category.id) : "category",
      );
    }
    if (action.target === "customCategoryDraftHost") {
      return t("settings.distractions.browser.websiteDeleteTitle", action.action.host);
    }
    const customCategory = findCustomCategory(action.action.customCategoryId);
    const name = customCategory?.name ?? t("settings.distractions.browser.customCategoryFallback");
    return action.action.type === "disable"
      ? t("settings.distractions.browser.allowCategoryTitle", name)
      : t("settings.distractions.browser.deleteCategoryTitle", name);
  }

  function pendingActionMessage(action: PendingAction): string {
    if (action.target === "website") {
      return action.action.type === "disable"
        ? t("settings.distractions.browser.websiteDisableMessage")
        : t("settings.distractions.shared.cannotBeUndone");
    }
    if (action.target === "browserConfiguration") {
      if (action.action.toggle === "enabled") {
        return t("settings.distractions.browser.browserOffMessage");
      }
      if (action.action.toggle === "focus") {
        return t("settings.distractions.browser.focusOffMessage");
      }
      if (action.action.toggle === "shortBreaks") {
        return t("settings.distractions.browser.shortBreaksOffMessage");
      }
      if (action.action.toggle === "longBreaks") {
        return t("settings.distractions.browser.longBreaksOffMessage");
      }
      return t("settings.distractions.browser.pauseActiveMessage");
    }
    if (action.target === "mode") {
      return t("settings.distractions.browser.whitelistWarning");
    }
    if (action.target === "category") {
      return t("settings.distractions.browser.categoryDisableMessage");
    }
    if (action.target === "customCategoryDraftHost") {
      return t("settings.distractions.shared.cannotBeUndone");
    }
    return action.action.type === "disable"
      ? t("settings.distractions.browser.categoryDisableMessage")
      : t("settings.distractions.shared.cannotBeUndone");
  }

  function pendingActionConfirmLabel(action: PendingAction): string {
    if (action.target === "website") {
      return action.action.type === "disable"
        ? t("settings.distractions.shared.disableAction")
        : t("settings.distractions.shared.deleteAction");
    }
    if (action.target === "browserConfiguration") {
      return action.action.toggle === "enabled"
        ? t("settings.distractions.shared.turnOffAction")
        : t("settings.distractions.shared.allowAction");
    }
    if (action.target === "mode") return t("settings.distractions.shared.switchAction");
    if (action.target === "category") return t("settings.distractions.shared.disableAction");
    if (action.target === "customCategoryDraftHost") return t("settings.distractions.shared.deleteAction");
    return action.action.type === "disable"
      ? t("settings.distractions.shared.disableAction")
      : t("settings.distractions.shared.deleteAction");
  }

</script>

{#snippet modeWebsiteSection(title: string, sections: readonly WebsiteListSection[])}
  <section class="flex flex-col gap-4">
    <h2 class="px-1 text-[0.866667rem] font-semibold text-foreground">{title}</h2>
    <div class="flex flex-col gap-4">
      {#each sections as section (section.kind)}
        <DistractionsRuleList
          id={section.id}
          heading={section.heading}
          description={section.description}
          placeholder={section.placeholder}
          emptyText={section.emptyText}
          errorText={section.errorText}
          items={websiteItems(section)}
          onAdd={section.add}
          onEnabledChange={(host, enabled) => requestHostEnabledChange(section, host, enabled)}
          onDelete={(host) => requestHostDelete(section, host)}
        />
      {/each}
    </div>
  </section>
{/snippet}

{#snippet blacklistModeSection()}
  <section class="flex flex-col gap-4">
    <h2 class="px-1 text-[0.866667rem] font-semibold text-foreground">{t("settings.distractions.browser.blacklistMode")}</h2>
    <div class="flex flex-col gap-4">
      {@render blockedCategoriesSubsection()}
      {#each blacklistWebsiteSections as section (section.kind)}
        <DistractionsRuleList
          id={section.id}
          heading={section.heading}
          description={section.description}
          placeholder={section.placeholder}
          emptyText={section.emptyText}
          errorText={section.errorText}
          items={websiteItems(section)}
          onAdd={section.add}
          onEnabledChange={(host, enabled) => requestHostEnabledChange(section, host, enabled)}
          onDelete={(host) => requestHostDelete(section, host)}
        />
      {/each}
    </div>
  </section>
{/snippet}

{#snippet blockedCategoriesSubsection()}
  <div class="flex flex-col gap-2 px-1 py-1">
    <div class="min-w-0">
      <h3 class="text-[0.866667rem] text-foreground">{t("settings.distractions.browser.blockedCategories")}</h3>
      <div class="mt-0.5 text-[0.8rem] text-muted-foreground">
        {t("settings.distractions.browser.blockedCategoriesDescription")}
      </div>
    </div>

    <div class="flex flex-wrap gap-2 py-1.5">
      {#each DISTRACTIONS_CATEGORY_DEFINITIONS as category (category.id)}
        {@const enabled = categoryEnabled(category.id)}
        <button
          type="button"
          onclick={() => requestCategoryEnabledChange(category.id, !enabled)}
          aria-label={enabled
            ? t("settings.distractions.browser.categoryEnabled", categoryLabel(category.id))
            : t("settings.distractions.browser.categoryDisabled", categoryLabel(category.id))}
          aria-pressed={enabled}
          class={cn(
            "inline-flex min-h-8 max-w-full items-center justify-center rounded-full border px-3 py-1.5 text-[0.8rem] font-medium leading-5",
            enabled
              ? "border-foreground/25 bg-foreground/5 text-foreground"
              : "border-border bg-transparent text-muted-foreground",
          )}
        >
          <span class="truncate">{categoryLabel(category.id)}</span>
        </button>
      {/each}

      {#each distractions.customCategories as customCategory (customCategory.id)}
        <span
          class={cn(
            "inline-flex max-w-full overflow-hidden rounded-full border",
            customCategory.enabled
              ? "border-foreground/25 bg-foreground/5 text-foreground"
              : "border-border bg-transparent text-muted-foreground",
          )}
          role="group"
          aria-label={t(
            "settings.distractions.browser.customCategoryState",
            customCategory.name,
            customCategory.enabled
              ? t("settings.distractions.shared.enabled")
              : t("settings.distractions.shared.disabled"),
          )}
        >
          <button
            type="button"
            onclick={() => requestCustomCategoryEnabledChange(customCategory, !customCategory.enabled)}
            aria-pressed={customCategory.enabled}
            class="inline-flex min-h-8 min-w-0 max-w-full items-center justify-center py-1.5 pl-3 pr-1 text-[0.8rem] font-medium leading-5"
          >
            <span class="truncate">{customCategory.name}</span>
          </button>
          <button
            type="button"
            onclick={() => openEditCustomCategoryForm(customCategory)}
            aria-label={t("settings.distractions.shared.edit", customCategory.name)}
            data-app-tooltip-disabled="true"
            class="flex min-h-8 w-7 shrink-0 items-center justify-center pr-2 text-muted-foreground"
          >
            <Pencil size={12} strokeWidth={2} />
          </button>
        </span>
      {/each}

      <button
        type="button"
        onclick={openNewCustomCategoryForm}
        aria-expanded={customCategoryFormOpen}
        class={cn(
          "inline-flex min-h-8 max-w-full items-center justify-center rounded-full border border-dashed px-3 py-1.5 text-[0.8rem] font-medium leading-5",
          customCategoryFormOpen && !customCategoryEditingId
            ? "border-foreground/25 bg-foreground/5 text-foreground"
            : "border-border bg-transparent text-muted-foreground",
        )}
      >
        <Plus size={13} strokeWidth={2.25} class="shrink-0" />
        <span class="ml-1.5 truncate">{t("settings.distractions.browser.newCategory")}</span>
      </button>
    </div>

    {#if customCategoryFormOpen}
      <div
        class="flex min-w-0 flex-col gap-1 py-1"
      >
        <div class="flex min-w-0 flex-wrap items-center gap-2">
          <label for="distractions-custom-category-name" class="sr-only">{t("settings.distractions.browser.categoryName")}</label>
          <input
            id="distractions-custom-category-name"
            bind:value={customCategoryDraft.name}
            oninput={() => clearCustomCategoryError("name")}
            onkeydown={handleCustomCategoryNameKeydown}
            type="text"
            spellcheck="false"
            placeholder={t("settings.distractions.browser.categoryName")}
            class="field flex h-7 min-w-32 flex-1 px-2 text-[0.8rem] leading-snug text-foreground"
          />
          <button
            type="button"
            onclick={saveCustomCategoryWithPendingHost}
            disabled={!customCategoryDraft.name.trim() || customCategoryDraft.hosts.length === 0}
            class="flex h-7 shrink-0 items-center justify-center gap-1.5 rounded-md border border-border bg-card px-2.5 text-[0.8rem] text-foreground transition-colors hover:bg-accent disabled:cursor-not-allowed disabled:opacity-40 disabled:hover:bg-card dark:bg-transparent dark:disabled:hover:bg-transparent"
          >
            {#if customCategoryEditingId}
              <Save size={13} strokeWidth={2.25} />
              <span>{t("settings.distractions.browser.save")}</span>
            {:else}
              <Plus size={13} strokeWidth={2.25} />
              <span>{t("settings.distractions.browser.add")}</span>
            {/if}
          </button>
          {#if customCategoryEditingId}
            <button
              type="button"
              onclick={requestEditingCustomCategoryDelete}
              class="flex h-7 shrink-0 items-center justify-center gap-1.5 rounded-md border border-border bg-card px-2.5 text-[0.8rem] text-foreground transition-colors hover:bg-accent dark:bg-transparent"
            >
              <Trash2 size={13} strokeWidth={2} />
              <span>{t("settings.distractions.browser.delete")}</span>
            </button>
          {/if}
          <button
            type="button"
            onclick={closeCustomCategoryForm}
            class="flex h-7 shrink-0 items-center justify-center rounded-md border border-border bg-card px-2.5 text-[0.8rem] text-foreground transition-colors hover:bg-accent dark:bg-transparent"
          >
            {t("settings.distractions.browser.cancel")}
          </button>
        </div>

        <div class="grid min-w-0">
          <div class="min-w-0">
            <form
              class="flex min-w-0 items-center gap-2 border-b border-border/70 py-1.5"
              onsubmit={submitCustomCategoryDraftHost}
            >
              <input
                id="distractions-custom-category-hosts"
                bind:value={customCategoryDraft.hostInput}
                oninput={() => clearCustomCategoryError("hosts")}
                type="text"
                spellcheck="false"
                placeholder={t("settings.distractions.browser.enterDomain")}
                class="flex h-7 min-w-0 flex-1 bg-transparent px-1 text-[0.8rem] leading-snug text-foreground outline-none placeholder:text-muted-foreground"
              />
              <button
                type="submit"
                disabled={!customCategoryDraft.hostInput.trim()}
                class="flex h-7 shrink-0 items-center justify-center gap-1.5 px-1 text-[0.8rem] font-medium text-muted-foreground transition-colors hover:text-foreground disabled:cursor-not-allowed disabled:opacity-40"
              >
                <Plus size={13} strokeWidth={2.25} />
                <span>{t("settings.distractions.browser.add")}</span>
              </button>
            </form>
            <div class="flex flex-col">
              {#each customCategoryDraft.hosts as host (host)}
                <div class="flex min-w-0 items-center gap-2 border-b border-border/70 py-1.5">
                  <span class="flex h-7 min-w-0 flex-1 items-center truncate px-1 text-[0.8rem] leading-snug text-foreground">
                    {host}
                  </span>
                  <button
                    type="button"
                    onclick={() => requestCustomCategoryDraftHostDelete(host)}
                    aria-label={t("settings.distractions.shared.remove", host)}
                    data-app-tooltip-disabled="true"
                    class="flex h-7 w-7 shrink-0 items-center justify-center rounded-md border border-border bg-card text-foreground transition-colors hover:bg-accent dark:bg-transparent"
                  >
                    <Trash2 size={13} strokeWidth={2} />
                  </button>
                </div>
              {:else}
                <div class="flex h-10 items-center border-b border-border/70 px-1 text-[0.8rem] text-muted-foreground">
                  {t("settings.distractions.browser.noWebsitesAdded")}
                </div>
              {/each}
            </div>
          </div>
        </div>
        {#if customCategoryErrors.name}
          <div class="text-[0.8rem] text-destructive">{customCategoryErrors.name}</div>
        {/if}
        {#if customCategoryErrors.hosts}
          <div class="text-[0.8rem] text-destructive">{customCategoryErrors.hosts}</div>
        {/if}
      </div>
    {/if}
  </div>
{/snippet}

<div class="flex flex-col gap-6">
  {#if showConnectionStatus}<DistractionsBrowserConnectionStatus />{/if}

  <DistractionsConfigurationSection
    title={t("settings.distractions.browser.browserConfiguration")}
    enabled={distractions.enabled}
    blockDuringFocus={distractions.blockDuringFocus}
    blockDuringShortBreaks={distractions.blockDuringShortBreaks}
    blockDuringLongBreaks={distractions.blockDuringLongBreaks}
    pauseDuringFocusPause={distractions.pauseDuringFocusPause}
    mode={distractions.mode}
    enabledLabel={t("settings.distractions.browser.enableBrowserBlocking")}
    enabledDescription={t("settings.distractions.browser.enableBrowserBlockingDescription")}
    focusDescription={t("settings.distractions.browser.focusDescription")}
    shortBreakDescription={t("settings.distractions.browser.shortBreakDescription")}
    longBreakDescription={t("settings.distractions.browser.longBreakDescription")}
    pauseDescription={t("settings.distractions.browser.pauseDescription")}
    modeHeading={t("settings.distractions.shared.websiteMode")}
    modeDescription={t("settings.distractions.browser.modeDescription")}
    blacklistDescription={t("settings.distractions.shared.blacklistDescription")}
    whitelistDescription={t("settings.distractions.shared.whitelistDescription")}
    onScheduleChange={requestBrowserConfigurationToggleChange}
    onModeChange={requestModeChange}
  />

  <fieldset
    disabled={!distractions.enabled}
    aria-disabled={!distractions.enabled}
    class={cn(
      "m-0 flex min-w-0 flex-col gap-6 border-0 p-0 transition-opacity",
      !distractions.enabled && "opacity-50",
    )}
  >
    <div class="h-px shrink-0 scale-y-50 bg-border" aria-hidden="true"></div>

    {#if distractions.mode === "blacklist"}
      {@render blacklistModeSection()}
    {:else}
      {@render modeWebsiteSection(t("settings.distractions.browser.whitelistMode"), whitelistWebsiteSections)}
    {/if}
  </fieldset>
</div>

{#if pendingAction}
  <ConfirmDialog
    title={pendingActionTitle(pendingAction)}
    message={pendingActionMessage(pendingAction)}
    confirmLabel={pendingActionConfirmLabel(pendingAction)}
    cancelLabel={t("settings.distractions.shared.cancelAction")}
    onConfirm={confirmPendingAction}
    onCancel={cancelPendingAction}
  />
{/if}
