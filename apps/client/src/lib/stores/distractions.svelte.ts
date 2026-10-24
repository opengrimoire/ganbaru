import {
  isProtectedDistractionsDesktopAppName,
  distractionsLimitEntrySourceKeys,
  normalizeDistractionsAppName,
  normalizeDistractionsConfig,
  normalizeDistractionsCustomCategoryName,
  normalizeDistractionsDesktopAppMatchNames,
  normalizeDistractionsLimitEntryColor,
  normalizeDistractionsLimitEntryName,
  normalizeDistractionsMobilePackage,
  normalizeDistractionsUsageLimitName,
  normalizeDistractionsHost,
  parseDistractionsHosts,
  type DistractionsAppRule,
  type DistractionsCategoryId,
  type DistractionsCategoryRule,
  type DistractionsConfig,
  type DistractionsCustomCategory,
  type DistractionsDesktopConfig,
  type DistractionsHostList,
  type DistractionsHostRule,
  type DistractionsLimitEntry,
  type DistractionsMobileAppRule,
  type DistractionsMobileConfig,
  type DistractionsMode,
  type DistractionsRuleTarget,
  type DistractionsRulesEditOutcome,
  type DistractionsSettingFlag,
  type DistractionsUsageLimit,
} from "$lib/distractions";
import {
  addDistractionsDesktopApp,
  addDistractionsHosts,
  addDistractionsMobileApp,
  loadDistractionsRules,
  removeDistractionsRule,
  resetDistractionsRules,
  saveDistractionsCustomCategory,
  saveDistractionsUsageLimit,
  setDistractionsRuleEnabled,
  setDistractionsRulesMode,
  type DistractionsUsageLimitDraft,
  type DistractionsUsageLimitEntryDraft,
} from "$lib/api/distractions";
import { listenSyncApplied } from "$lib/api/sync";
import { publishMobileDistractionsConfig } from "$lib/scheduling/mobile-distractions";
import { createVaultChangeChannel } from "$lib/windows/vault-change-channel";

export type { DistractionsUsageLimitDraft, DistractionsUsageLimitEntryDraft } from "$lib/api/distractions";

/** Change key other windows receive after a rule edit; the whole rule set reloads. */
const RULES_CHANGE_KEY = "rules";

/** Sync applied topic and tables that carry distraction rules. */
const RULES_SYNC_TABLES: ReadonlySet<string> = new Set([
  "distractions_rules",
  "distractions_settings",
  "distractions_category_rules",
  "distractions_custom_categories",
  "distractions_host_rules",
  "distractions_app_rules",
  "distractions_usage_limits",
  "distractions_usage_limit_entries",
]);

export type AddDistractionsCustomCategoryResult =
  | "added"
  | "invalid-name"
  | "invalid-hosts"
  | "duplicate-name";

export type UpdateDistractionsCustomCategoryResult =
  | "updated"
  | "missing"
  | "invalid-name"
  | "invalid-hosts"
  | "duplicate-name";

export type SaveDistractionsUsageLimitResult =
  | "saved"
  | "missing"
  | "invalid-name"
  | "invalid-minutes"
  | "invalid-budget"
  | "invalid-sources"
  | "duplicate-source"
  | "protected-source";

let config = $state<DistractionsConfig>(normalizeDistractionsConfig(undefined));

/** Edits and reloads run one at a time, in the order they were made. */
let queue: Promise<void> = Promise.resolve();
/** Increments with every local edit, so only the newest edit's or a later reload's rules replace what is shown. */
let editGeneration = 0;

const ruleChanges = createVaultChangeChannel("distractions-rules-changed");

/** Replace the shown rules with the stored ones unless a local edit started meanwhile. */
async function reloadRules(): Promise<void> {
  const generation = editGeneration;
  try {
    const next = await loadDistractionsRules();
    if (generation === editGeneration) config = next;
  } catch (error) {
    console.warn("Distractions rules could not be loaded", error);
  }
}

function queueReload(): void {
  queue = queue.then(reloadRules);
}

/**
 * Show an edit at once, then store it through its native command. The command's rules replace the shown ones when no
 * newer edit is pending; a refusal or failure reloads the stored rules, so the screen never keeps an unsaved change.
 */
function edit(next: DistractionsConfig | null, run: () => Promise<DistractionsRulesEditOutcome>): void {
  if (next) config = next;
  editGeneration += 1;
  const generation = editGeneration;
  queue = queue.then(async () => {
    try {
      const outcome = await run();
      if (outcome.status === "saved") {
        ruleChanges.publish([RULES_CHANGE_KEY]);
        if (generation === editGeneration) config = outcome.config;
        return;
      }
      console.warn("Distractions rule edit was refused", outcome.reason);
    } catch (error) {
      console.warn("Distractions rule edit could not be saved", error);
    }
    if (generation === editGeneration) await reloadRules();
  });
}

function update(partial: Partial<DistractionsConfig>, run: () => Promise<DistractionsRulesEditOutcome>): void {
  edit({ ...config, ...partial }, run);
}

function updateDesktop(
  partial: Partial<DistractionsDesktopConfig>,
  run: () => Promise<DistractionsRulesEditOutcome>,
): void {
  update({ desktop: { ...config.desktop, ...partial } }, run);
}

function updateMobile(
  partial: Partial<DistractionsMobileConfig>,
  run: () => Promise<DistractionsRulesEditOutcome>,
): void {
  update({ mobile: { ...config.mobile, ...partial } }, run);
}

function updateLimits(
  partial: Partial<DistractionsConfig["limits"]>,
  run: () => Promise<DistractionsRulesEditOutcome>,
): void {
  update({ limits: { ...config.limits, ...partial } }, run);
}

function setSetting(setting: DistractionsSettingFlag, enabled: boolean): () => Promise<DistractionsRulesEditOutcome> {
  return () => setDistractionsRuleEnabled({ kind: "setting", setting }, enabled);
}

function setTarget(target: DistractionsRuleTarget, enabled: boolean): () => Promise<DistractionsRulesEditOutcome> {
  return () => setDistractionsRuleEnabled(target, enabled);
}

function removeTarget(target: DistractionsRuleTarget): () => Promise<DistractionsRulesEditOutcome> {
  return () => removeDistractionsRule(target);
}

function hostListKey(list: DistractionsHostList): "blockedHosts" | "exceptionHosts" | "allowedHosts" {
  if (list === "blocked") return "blockedHosts";
  return list === "exception" ? "exceptionHosts" : "allowedHosts";
}

function addHostsText(list: DistractionsHostList, text: string): boolean {
  const key = hostListKey(list);
  const merged = mergeHosts(config[key], text);
  if (!merged) return false;
  const hosts = parseDistractionsHosts(text);
  update({ [key]: merged }, () => addDistractionsHosts(list, hosts));
  return true;
}

function removeListedHost(list: DistractionsHostList, host: string): void {
  const key = hostListKey(list);
  update({ [key]: removeHost(config[key], host) }, removeTarget({ kind: "host", list, host }));
}

function setListedHostEnabled(list: DistractionsHostList, host: string, enabled: boolean): void {
  const key = hostListKey(list);
  update({ [key]: setHostEnabled(config[key], host, enabled) }, setTarget({ kind: "host", list, host }, enabled));
}

/** Send budgets as whole minutes, the unit the native rules store. */
function wholeMinuteDraft(draft: DistractionsUsageLimitDraft): DistractionsUsageLimitDraft {
  return {
    ...draft,
    minutesPerDay: draft.minutesPerDay === null ? null : Math.trunc(draft.minutesPerDay),
    minutesPerWeek: draft.minutesPerWeek === null ? null : Math.trunc(draft.minutesPerWeek),
  };
}

function startRulesListeners(): void {
  void listenSyncApplied((tables) => {
    if (tables.some((table) => RULES_SYNC_TABLES.has(table))) queueReload();
  }).catch((error: unknown) => console.warn("Distractions rules could not listen for sync", error));
  void ruleChanges.listen(() => queueReload())
    .catch((error: unknown) => console.warn("Distractions rules could not listen for other windows", error));
}

startRulesListeners();
queueReload();

function mergeHosts(
  existingHosts: readonly DistractionsHostRule[],
  input: string,
): DistractionsHostRule[] | null {
  const hosts = parseDistractionsHosts(input);
  if (hosts.length === 0) return null;
  const seen = new Set(existingHosts.map((rule) => rule.host));
  const merged = [...existingHosts];
  for (const host of hosts) {
    if (seen.has(host)) continue;
    seen.add(host);
    merged.push({ host, enabled: true });
  }
  return merged;
}

function removeHost(
  existingHosts: readonly DistractionsHostRule[],
  host: string,
): DistractionsHostRule[] {
  return existingHosts.filter((rule) => rule.host !== host);
}

function setHostEnabled(
  existingHosts: readonly DistractionsHostRule[],
  host: string,
  enabled: boolean,
): DistractionsHostRule[] {
  return existingHosts.map((rule) => rule.host === host ? { ...rule, enabled } : rule);
}

function appRuleKey(name: string): string {
  return name.toLowerCase();
}

interface DistractionsAppRuleInput {
  name: string;
  matchNames?: readonly string[];
}

function appRuleFromInput(
  input: string | DistractionsAppRuleInput,
): DistractionsAppRule | null {
  const name = normalizeDistractionsAppName(typeof input === "string" ? input : input.name);
  if (!name || isProtectedDistractionsDesktopAppName(name)) return null;
  const seen = new Set<string>();
  const matchNames: string[] = [];
  const rawMatchNames = typeof input === "string" ? [name] : [name, ...(input.matchNames ?? [])];
  for (const rawMatchName of rawMatchNames) {
    const matchName = normalizeDistractionsAppName(rawMatchName);
    if (!matchName || isProtectedDistractionsDesktopAppName(matchName)) continue;
    const key = appRuleKey(matchName);
    if (seen.has(key)) continue;
    seen.add(key);
    matchNames.push(matchName);
  }
  return {
    name,
    enabled: true,
    matchNames: matchNames.length > 0 ? matchNames : [name],
  };
}

function mergeApps(
  existingApps: readonly DistractionsAppRule[],
  input: string | DistractionsAppRuleInput,
): DistractionsAppRule[] | null {
  const rule = appRuleFromInput(input);
  if (!rule) return null;
  const key = appRuleKey(rule.name);
  if (existingApps.some((rule) => appRuleKey(rule.name) === key)) return existingApps.slice();
  return [...existingApps, rule];
}

function removeApp(
  existingApps: readonly DistractionsAppRule[],
  name: string,
): DistractionsAppRule[] {
  const key = appRuleKey(name);
  return existingApps.filter((rule) => appRuleKey(rule.name) !== key);
}

function setAppEnabled(
  existingApps: readonly DistractionsAppRule[],
  name: string,
  enabled: boolean,
): DistractionsAppRule[] {
  if (isProtectedDistractionsDesktopAppName(name)) return existingApps.slice();
  const key = appRuleKey(name);
  return existingApps.map((rule) => appRuleKey(rule.name) === key ? { ...rule, enabled } : rule);
}

function setCategoryEnabled(
  categories: readonly DistractionsCategoryRule[],
  id: DistractionsCategoryId,
  enabled: boolean,
): DistractionsCategoryRule[] {
  return categories.map((rule) => rule.id === id ? { ...rule, enabled } : rule);
}

function setCustomCategoryEnabled(
  customCategories: readonly DistractionsCustomCategory[],
  id: string,
  enabled: boolean,
): DistractionsCustomCategory[] {
  return customCategories.map((customCategory) => customCategory.id === id ? { ...customCategory, enabled } : customCategory);
}

function removeCustomCategory(
  customCategories: readonly DistractionsCustomCategory[],
  id: string,
): DistractionsCustomCategory[] {
  return customCategories.filter((customCategory) => customCategory.id !== id);
}

function isDuplicateCustomCategoryName(name: string, currentId: string | null): boolean {
  const normalizedName = name.toLowerCase();
  return config.customCategoryStacks.some((customCategory) => (
    customCategory.id !== currentId && customCategory.name.toLowerCase() === normalizedName
  ));
}

function normalizeUsageLimitEntryId(value: string): string | null {
  const id = value.trim();
  if (!/^[a-z0-9][a-z0-9_-]{0,79}$/i.test(id)) return null;
  return id;
}

function websiteDisplayName(host: string): string {
  const firstLabel = host.replace(/^www\./, "").split(".", 1)[0] ?? host;
  if (!firstLabel) return host;
  return `${firstLabel.charAt(0).toUpperCase()}${firstLabel.slice(1)}`;
}

function derivedEntryName(entry: DistractionsLimitEntry): string {
  return entry.name
    ?? entry.mobileAppName
    ?? entry.desktopAppName
    ?? (entry.websiteHost ? websiteDisplayName(entry.websiteHost) : "Daily limit");
}

function normalizeLimitDraftEntry(
  draft: DistractionsUsageLimitEntryDraft,
): DistractionsLimitEntry | "invalid-sources" | "protected-source" {
  const id = normalizeUsageLimitEntryId(draft.id);
  if (!id) return "invalid-sources";
  const name = normalizeDistractionsLimitEntryName(draft.name);
  const websiteHost = draft.websiteHost.trim()
    ? normalizeDistractionsHost(draft.websiteHost)
    : null;
  const mobileAppName = draft.mobileAppName.trim()
    ? normalizeDistractionsAppName(draft.mobileAppName)
    : null;
  const mobileAppPackage = draft.mobileAppPackage.trim()
    ? normalizeDistractionsMobilePackage(draft.mobileAppPackage)
    : null;
  if (mobileAppPackage && !mobileAppName) return "invalid-sources";
  const desktopAppName = draft.desktopAppName.trim()
    ? normalizeDistractionsAppName(draft.desktopAppName)
    : null;
  const desktopAppMatchNames = desktopAppName
    ? normalizeDistractionsDesktopAppMatchNames(
      desktopAppName,
      draft.desktopAppMatchNames,
    )
    : [];
  if (!websiteHost && !mobileAppName && !desktopAppName) return "invalid-sources";
  if (desktopAppName && isProtectedDistractionsDesktopAppName(desktopAppName)) {
    return "protected-source";
  }
  if (desktopAppMatchNames.some(isProtectedDistractionsDesktopAppName)) {
    return "protected-source";
  }
  const color = normalizeDistractionsLimitEntryColor(draft.color);
  const entry: DistractionsLimitEntry = {
    id,
    name,
    websiteHost,
    mobileAppName,
    mobileAppPackage,
    desktopAppName,
    desktopAppMatchNames,
  };
  return color === null ? entry : { ...entry, color };
}

function normalizeLimitDraft(
  draft: DistractionsUsageLimitDraft,
): SaveDistractionsUsageLimitResult | Omit<DistractionsUsageLimit, "id" | "enabled"> {
  const minutesPerDay = draft.minutesPerDay === null ? null : Math.trunc(draft.minutesPerDay);
  const minutesPerWeek = draft.minutesPerWeek === null ? null : Math.trunc(draft.minutesPerWeek);
  if (
    minutesPerDay !== null
    && (!Number.isFinite(draft.minutesPerDay) || minutesPerDay < 1 || minutesPerDay > 24 * 60)
  ) {
    return "invalid-minutes";
  }
  if (
    minutesPerWeek !== null
    && (!Number.isFinite(draft.minutesPerWeek) || minutesPerWeek < 1 || minutesPerWeek > 7 * 24 * 60)
  ) {
    return "invalid-minutes";
  }
  if (minutesPerDay === null && minutesPerWeek === null) return "invalid-budget";
  if (draft.entries.length === 0) return "invalid-sources";
  const seenSources = new Set<string>();
  const seenEntryIds = new Set<string>();
  const entries: DistractionsLimitEntry[] = [];
  for (const draftEntry of draft.entries) {
    const entry = normalizeLimitDraftEntry(draftEntry);
    if (entry === "invalid-sources" || entry === "protected-source") return entry;
    if (seenEntryIds.has(entry.id)) return "duplicate-source";
    seenEntryIds.add(entry.id);
    for (const key of distractionsLimitEntrySourceKeys(entry)) {
      if (seenSources.has(key)) return "duplicate-source";
      seenSources.add(key);
    }
    entries.push(entry);
  }
  const firstEntry = entries[0];
  if (!firstEntry) return "invalid-sources";
  const explicitName = normalizeDistractionsUsageLimitName(draft.name);
  if (entries.length > 1 && !explicitName) return "invalid-name";
  const name = explicitName ?? derivedEntryName(firstEntry);
  if (!name) return "invalid-name";
  const normalized: Omit<DistractionsUsageLimit, "id" | "enabled"> = {
    name,
    minutesPerDay,
    entries,
  };
  return minutesPerWeek === null ? normalized : { ...normalized, minutesPerWeek };
}

export function getDistractions() {
  return {
    get enabled(): boolean {
      return config.enabled;
    },
    get mode(): DistractionsMode {
      return config.mode;
    },
    get blockDuringFocus(): boolean {
      return config.blockDuringFocus;
    },
    get blockDuringShortBreaks(): boolean {
      return config.blockDuringShortBreaks;
    },
    get blockDuringLongBreaks(): boolean {
      return config.blockDuringLongBreaks;
    },
    get pauseDuringFocusPause(): boolean {
      return config.pauseDuringFocusPause;
    },
    get blockedCategories(): readonly DistractionsCategoryRule[] {
      return config.blockedCategories;
    },
    get customCategories(): readonly DistractionsCustomCategory[] {
      return config.customCategoryStacks;
    },
    get blockedHosts(): readonly DistractionsHostRule[] {
      return config.blockedHosts;
    },
    get exceptionHosts(): readonly DistractionsHostRule[] {
      return config.exceptionHosts;
    },
    get allowedHosts(): readonly DistractionsHostRule[] {
      return config.allowedHosts;
    },
    get desktopEnabled(): boolean {
      return config.desktop.enabled;
    },
    get desktopBlockDuringFocus(): boolean {
      return config.desktop.blockDuringFocus;
    },
    get desktopBlockDuringShortBreaks(): boolean {
      return config.desktop.blockDuringShortBreaks;
    },
    get desktopBlockDuringLongBreaks(): boolean {
      return config.desktop.blockDuringLongBreaks;
    },
    get desktopPauseDuringFocusPause(): boolean {
      return config.desktop.pauseDuringFocusPause;
    },
    get blockedApps(): readonly DistractionsAppRule[] {
      return config.desktop.blockedApps;
    },
    get mobileEnabled(): boolean {
      return config.mobile.enabled;
    },
    get mobileBlockDuringFocus(): boolean {
      return config.mobile.blockDuringFocus;
    },
    get mobileBlockDuringShortBreaks(): boolean {
      return config.mobile.blockDuringShortBreaks;
    },
    get mobileBlockDuringLongBreaks(): boolean {
      return config.mobile.blockDuringLongBreaks;
    },
    get mobilePauseDuringFocusPause(): boolean {
      return config.mobile.pauseDuringFocusPause;
    },
    get blockedMobileApps(): readonly DistractionsMobileAppRule[] {
      return config.mobile.blockedApps;
    },
    get limitsEnabled(): boolean {
      return config.limits.enabled;
    },
    get usageLimits(): readonly DistractionsUsageLimit[] {
      return config.limits.items;
    },
    get config(): DistractionsConfig {
      return config;
    },
    publishMobileRules(): Promise<void> {
      return publishMobileDistractionsConfig();
    },
    setMode(mode: DistractionsMode): void {
      update({ mode }, () => setDistractionsRulesMode(mode));
    },
    setEnabled(enabled: boolean): void {
      update({ enabled }, setSetting("enabled", enabled));
    },
    setBlockDuringFocus(blockDuringFocus: boolean): void {
      update({ blockDuringFocus }, setSetting("blockDuringFocus", blockDuringFocus));
    },
    setBlockDuringShortBreaks(blockDuringShortBreaks: boolean): void {
      update({ blockDuringShortBreaks }, setSetting("blockDuringShortBreaks", blockDuringShortBreaks));
    },
    setBlockDuringLongBreaks(blockDuringLongBreaks: boolean): void {
      update({ blockDuringLongBreaks }, setSetting("blockDuringLongBreaks", blockDuringLongBreaks));
    },
    setPauseDuringFocusPause(pauseDuringFocusPause: boolean): void {
      update({ pauseDuringFocusPause }, setSetting("pauseDuringFocusPause", pauseDuringFocusPause));
    },
    setDesktopEnabled(enabled: boolean): void {
      updateDesktop({ enabled }, setSetting("desktopEnabled", enabled));
    },
    setDesktopBlockDuringFocus(blockDuringFocus: boolean): void {
      updateDesktop({ blockDuringFocus }, setSetting("desktopBlockDuringFocus", blockDuringFocus));
    },
    setDesktopBlockDuringShortBreaks(blockDuringShortBreaks: boolean): void {
      updateDesktop({ blockDuringShortBreaks }, setSetting("desktopBlockDuringShortBreaks", blockDuringShortBreaks));
    },
    setDesktopBlockDuringLongBreaks(blockDuringLongBreaks: boolean): void {
      updateDesktop({ blockDuringLongBreaks }, setSetting("desktopBlockDuringLongBreaks", blockDuringLongBreaks));
    },
    setDesktopPauseDuringFocusPause(pauseDuringFocusPause: boolean): void {
      updateDesktop({ pauseDuringFocusPause }, setSetting("desktopPauseDuringFocusPause", pauseDuringFocusPause));
    },
    setMobileEnabled(enabled: boolean): void {
      updateMobile({ enabled }, setSetting("mobileEnabled", enabled));
    },
    setMobileBlockDuringFocus(blockDuringFocus: boolean): void {
      updateMobile({ blockDuringFocus }, setSetting("mobileBlockDuringFocus", blockDuringFocus));
    },
    setMobileBlockDuringShortBreaks(blockDuringShortBreaks: boolean): void {
      updateMobile({ blockDuringShortBreaks }, setSetting("mobileBlockDuringShortBreaks", blockDuringShortBreaks));
    },
    setMobileBlockDuringLongBreaks(blockDuringLongBreaks: boolean): void {
      updateMobile({ blockDuringLongBreaks }, setSetting("mobileBlockDuringLongBreaks", blockDuringLongBreaks));
    },
    setMobilePauseDuringFocusPause(pauseDuringFocusPause: boolean): void {
      updateMobile({ pauseDuringFocusPause }, setSetting("mobilePauseDuringFocusPause", pauseDuringFocusPause));
    },
    setLimitsEnabled(enabled: boolean): void {
      updateLimits({ enabled }, setSetting("limitsEnabled", enabled));
    },
    setBlockedCategoryEnabled(id: DistractionsCategoryId, enabled: boolean): void {
      update(
        { blockedCategories: setCategoryEnabled(config.blockedCategories, id, enabled) },
        setTarget({ kind: "category", id }, enabled),
      );
    },
    addCustomCategory(
      nameInput: string,
      hostsInput: string,
    ): AddDistractionsCustomCategoryResult {
      const name = normalizeDistractionsCustomCategoryName(nameInput);
      if (!name) return "invalid-name";
      const hosts = parseDistractionsHosts(hostsInput);
      if (hosts.length === 0) return "invalid-hosts";
      if (isDuplicateCustomCategoryName(name, null)) return "duplicate-name";
      edit(null, () => saveDistractionsCustomCategory(null, name, hosts));
      return "added";
    },
    updateCustomCategory(
      id: string,
      nameInput: string,
      hostsInput: string,
    ): UpdateDistractionsCustomCategoryResult {
      const name = normalizeDistractionsCustomCategoryName(nameInput);
      if (!name) return "invalid-name";
      const hosts = parseDistractionsHosts(hostsInput);
      if (hosts.length === 0) return "invalid-hosts";
      if (isDuplicateCustomCategoryName(name, id)) return "duplicate-name";
      let found = false;
      const customCategoryStacks = config.customCategoryStacks.map((customCategory) => {
        if (customCategory.id !== id) return customCategory;
        found = true;
        const enabledByHost = new Map(customCategory.hosts.map((rule) => [rule.host, rule.enabled]));
        return {
          ...customCategory,
          name,
          hosts: hosts.map((host) => ({ host, enabled: enabledByHost.get(host) ?? true })),
        };
      });
      if (!found) return "missing";
      update({ customCategoryStacks }, () => saveDistractionsCustomCategory(id, name, hosts));
      return "updated";
    },
    removeCustomCategory(id: string): void {
      update(
        { customCategoryStacks: removeCustomCategory(config.customCategoryStacks, id) },
        removeTarget({ kind: "customCategory", id }),
      );
    },
    setCustomCategoryEnabled(id: string, enabled: boolean): void {
      update(
        { customCategoryStacks: setCustomCategoryEnabled(config.customCategoryStacks, id, enabled) },
        setTarget({ kind: "customCategory", id }, enabled),
      );
    },
    addBlockedHostsText(text: string): boolean {
      return addHostsText("blocked", text);
    },
    addExceptionHostsText(text: string): boolean {
      return addHostsText("exception", text);
    },
    addAllowedHostsText(text: string): boolean {
      return addHostsText("allowed", text);
    },
    addBlockedAppsText(text: string): boolean {
      const rule = appRuleFromInput(text);
      const merged = mergeApps(config.desktop.blockedApps, text);
      if (!rule || !merged) return false;
      updateDesktop({ blockedApps: merged }, () => addDistractionsDesktopApp(rule.name, rule.matchNames));
      return true;
    },
    addBlockedApp(name: string, matchNames: readonly string[]): boolean {
      const rule = appRuleFromInput({ name, matchNames });
      const merged = mergeApps(config.desktop.blockedApps, { name, matchNames });
      if (!rule || !merged) return false;
      updateDesktop({ blockedApps: merged }, () => addDistractionsDesktopApp(rule.name, rule.matchNames));
      return true;
    },
    addBlockedMobileApp(nameInput: string, packageInput: string): boolean {
      const name = normalizeDistractionsAppName(nameInput);
      const packageName = normalizeDistractionsMobilePackage(packageInput);
      if (!name || !packageName) return false;
      if (config.mobile.blockedApps.some((rule) => (
        rule.packageName.toLowerCase() === packageName.toLowerCase()
      ))) return true;
      updateMobile(
        { blockedApps: [...config.mobile.blockedApps, { name, packageName, enabled: true }] },
        () => addDistractionsMobileApp(name, packageName),
      );
      return true;
    },
    removeBlockedHost(host: string): void {
      removeListedHost("blocked", host);
    },
    removeExceptionHost(host: string): void {
      removeListedHost("exception", host);
    },
    removeAllowedHost(host: string): void {
      removeListedHost("allowed", host);
    },
    removeBlockedApp(name: string): void {
      updateDesktop(
        { blockedApps: removeApp(config.desktop.blockedApps, name) },
        removeTarget({ kind: "desktopApp", name }),
      );
    },
    removeBlockedMobileApp(packageName: string): void {
      const key = packageName.toLowerCase();
      updateMobile(
        { blockedApps: config.mobile.blockedApps.filter((rule) => rule.packageName.toLowerCase() !== key) },
        removeTarget({ kind: "mobileApp", packageName }),
      );
    },
    setBlockedHostEnabled(host: string, enabled: boolean): void {
      setListedHostEnabled("blocked", host, enabled);
    },
    setExceptionHostEnabled(host: string, enabled: boolean): void {
      setListedHostEnabled("exception", host, enabled);
    },
    setAllowedHostEnabled(host: string, enabled: boolean): void {
      setListedHostEnabled("allowed", host, enabled);
    },
    setBlockedAppEnabled(name: string, enabled: boolean): void {
      if (isProtectedDistractionsDesktopAppName(name)) return;
      updateDesktop(
        { blockedApps: setAppEnabled(config.desktop.blockedApps, name, enabled) },
        setTarget({ kind: "desktopApp", name }, enabled),
      );
    },
    setBlockedMobileAppEnabled(packageName: string, enabled: boolean): void {
      const key = packageName.toLowerCase();
      updateMobile(
        {
          blockedApps: config.mobile.blockedApps.map((rule) => (
            rule.packageName.toLowerCase() === key ? { ...rule, enabled } : rule
          )),
        },
        setTarget({ kind: "mobileApp", packageName }, enabled),
      );
    },
    addUsageLimit(draft: DistractionsUsageLimitDraft): SaveDistractionsUsageLimitResult {
      const normalized = normalizeLimitDraft(draft);
      if (typeof normalized === "string") return normalized;
      edit(null, () => saveDistractionsUsageLimit(null, wholeMinuteDraft(draft)));
      return "saved";
    },
    updateUsageLimit(id: string, draft: DistractionsUsageLimitDraft): SaveDistractionsUsageLimitResult {
      const normalized = normalizeLimitDraft(draft);
      if (typeof normalized === "string") return normalized;
      let found = false;
      const items = config.limits.items.map((limit) => {
        if (limit.id !== id) return limit;
        found = true;
        return {
          ...limit,
          ...normalized,
        };
      });
      if (!found) return "missing";
      updateLimits({ items }, () => saveDistractionsUsageLimit(id, wholeMinuteDraft(draft)));
      return "saved";
    },
    removeUsageLimit(id: string): void {
      updateLimits(
        { items: config.limits.items.filter((limit) => limit.id !== id) },
        removeTarget({ kind: "usageLimit", id }),
      );
    },
    setUsageLimitEnabled(id: string, enabled: boolean): void {
      updateLimits(
        { items: config.limits.items.map((limit) => limit.id === id ? { ...limit, enabled } : limit) },
        setTarget({ kind: "usageLimit", id }, enabled),
      );
    },
    reset(): void {
      edit(normalizeDistractionsConfig(undefined), resetDistractionsRules);
    },
  };
}
