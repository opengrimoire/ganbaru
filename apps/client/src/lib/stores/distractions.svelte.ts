import {
  DEFAULT_DISTRACTIONS_CONFIG,
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
  type DistractionsHostRule,
  type DistractionsLimitEntry,
  type DistractionsMobileAppRule,
  type DistractionsMobileConfig,
  type DistractionsMode,
  type DistractionsUsageLimit,
} from "$lib/distractions";
import type { EventColor } from "$lib/calendar/types";
import { getConfigKey, setConfigKey } from "$lib/vault/config";
import { publishMobileDistractionsConfig } from "$lib/scheduling/mobile-distractions";

const CONFIG_KEY = "distractions";

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

export interface DistractionsUsageLimitDraft {
  name: string;
  minutesPerDay: number | null;
  minutesPerWeek: number | null;
  entries: readonly DistractionsUsageLimitEntryDraft[];
}

export interface DistractionsUsageLimitEntryDraft {
  id: string;
  name: string;
  color?: EventColor | null;
  websiteHost: string;
  mobileAppName: string;
  mobileAppPackage: string;
  desktopAppName: string;
  desktopAppMatchNames: readonly string[];
}

function loadSavedConfig(): DistractionsConfig {
  const saved = getConfigKey<unknown>(CONFIG_KEY, undefined);
  return normalizeDistractionsConfig(saved);
}

let config = $state<DistractionsConfig>(loadSavedConfig());

function persist(next: DistractionsConfig): void {
  config = next;
  setConfigKey(CONFIG_KEY, next);
  void publishMobileDistractionsConfig().catch((error: unknown) => {
    console.warn("Android Distractions rules could not be published", error);
  });
}

function update(partial: Partial<DistractionsConfig>): void {
  persist({ ...config, ...partial });
}

function updateDesktop(partial: Partial<DistractionsDesktopConfig>): void {
  update({ desktop: { ...config.desktop, ...partial } });
}

function updateMobile(partial: Partial<DistractionsMobileConfig>): void {
  update({ mobile: { ...config.mobile, ...partial } });
}

function updateLimits(partial: Partial<DistractionsConfig["limits"]>): void {
  update({ limits: { ...config.limits, ...partial } });
}

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

function slugifyCustomCategoryName(name: string): string {
  const slug = name
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
  return slug || "custom-stack";
}

function createCustomCategoryId(name: string): string {
  const suffix = typeof crypto !== "undefined" && typeof crypto.randomUUID === "function"
    ? crypto.randomUUID().replace(/-/g, "").slice(0, 12)
    : `${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
  return `${slugifyCustomCategoryName(name).slice(0, 48)}-${suffix}`;
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

function createUsageLimitId(name: string): string {
  const base = name
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 48) || "limit";
  const suffix = typeof crypto !== "undefined" && typeof crypto.randomUUID === "function"
    ? crypto.randomUUID().replace(/-/g, "").slice(0, 12)
    : `${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
  return `${base}-${suffix}`;
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
      update({ mode });
    },
    setEnabled(enabled: boolean): void {
      update({ enabled });
    },
    setBlockDuringFocus(blockDuringFocus: boolean): void {
      update({ blockDuringFocus });
    },
    setBlockDuringShortBreaks(blockDuringShortBreaks: boolean): void {
      update({ blockDuringShortBreaks });
    },
    setBlockDuringLongBreaks(blockDuringLongBreaks: boolean): void {
      update({ blockDuringLongBreaks });
    },
    setPauseDuringFocusPause(pauseDuringFocusPause: boolean): void {
      update({ pauseDuringFocusPause });
    },
    setDesktopEnabled(enabled: boolean): void {
      updateDesktop({ enabled });
    },
    setDesktopBlockDuringFocus(blockDuringFocus: boolean): void {
      updateDesktop({ blockDuringFocus });
    },
    setDesktopBlockDuringShortBreaks(blockDuringShortBreaks: boolean): void {
      updateDesktop({ blockDuringShortBreaks });
    },
    setDesktopBlockDuringLongBreaks(blockDuringLongBreaks: boolean): void {
      updateDesktop({ blockDuringLongBreaks });
    },
    setDesktopPauseDuringFocusPause(pauseDuringFocusPause: boolean): void {
      updateDesktop({ pauseDuringFocusPause });
    },
    setMobileEnabled(enabled: boolean): void {
      updateMobile({ enabled });
    },
    setMobileBlockDuringFocus(blockDuringFocus: boolean): void {
      updateMobile({ blockDuringFocus });
    },
    setMobileBlockDuringShortBreaks(blockDuringShortBreaks: boolean): void {
      updateMobile({ blockDuringShortBreaks });
    },
    setMobileBlockDuringLongBreaks(blockDuringLongBreaks: boolean): void {
      updateMobile({ blockDuringLongBreaks });
    },
    setMobilePauseDuringFocusPause(pauseDuringFocusPause: boolean): void {
      updateMobile({ pauseDuringFocusPause });
    },
    setLimitsEnabled(enabled: boolean): void {
      updateLimits({ enabled });
    },
    setBlockedCategoryEnabled(id: DistractionsCategoryId, enabled: boolean): void {
      update({ blockedCategories: setCategoryEnabled(config.blockedCategories, id, enabled) });
    },
    addCustomCategory(
      nameInput: string,
      hostsInput: string,
    ): AddDistractionsCustomCategoryResult {
      const name = normalizeDistractionsCustomCategoryName(nameInput);
      if (!name) return "invalid-name";
      const hosts = parseDistractionsHosts(hostsInput).map((host) => ({ host, enabled: true }));
      if (hosts.length === 0) return "invalid-hosts";
      if (isDuplicateCustomCategoryName(name, null)) return "duplicate-name";
      const existingIds = new Set(config.customCategoryStacks.map((customCategory) => customCategory.id));
      let id = createCustomCategoryId(name);
      while (existingIds.has(id)) {
        id = createCustomCategoryId(name);
      }
      update({
        customCategoryStacks: [
          ...config.customCategoryStacks,
          {
            id,
            name,
            enabled: true,
            hosts,
          },
        ],
      });
      return "added";
    },
    updateCustomCategory(
      id: string,
      nameInput: string,
      hostsInput: string,
    ): UpdateDistractionsCustomCategoryResult {
      const name = normalizeDistractionsCustomCategoryName(nameInput);
      if (!name) return "invalid-name";
      const hosts = parseDistractionsHosts(hostsInput).map((host) => ({ host, enabled: true }));
      if (hosts.length === 0) return "invalid-hosts";
      if (isDuplicateCustomCategoryName(name, id)) return "duplicate-name";
      let found = false;
      const customCategoryStacks = config.customCategoryStacks.map((customCategory) => {
        if (customCategory.id !== id) return customCategory;
        found = true;
        return {
          ...customCategory,
          name,
          hosts,
        };
      });
      if (!found) return "missing";
      update({ customCategoryStacks });
      return "updated";
    },
    removeCustomCategory(id: string): void {
      update({ customCategoryStacks: removeCustomCategory(config.customCategoryStacks, id) });
    },
    setCustomCategoryEnabled(id: string, enabled: boolean): void {
      update({
        customCategoryStacks: setCustomCategoryEnabled(config.customCategoryStacks, id, enabled),
      });
    },
    addBlockedHostsText(text: string): boolean {
      const merged = mergeHosts(config.blockedHosts, text);
      if (!merged) return false;
      update({ blockedHosts: merged });
      return true;
    },
    addExceptionHostsText(text: string): boolean {
      const merged = mergeHosts(config.exceptionHosts, text);
      if (!merged) return false;
      update({ exceptionHosts: merged });
      return true;
    },
    addAllowedHostsText(text: string): boolean {
      const merged = mergeHosts(config.allowedHosts, text);
      if (!merged) return false;
      update({ allowedHosts: merged });
      return true;
    },
    addBlockedAppsText(text: string): boolean {
      const merged = mergeApps(config.desktop.blockedApps, text);
      if (!merged) return false;
      updateDesktop({ blockedApps: merged });
      return true;
    },
    addBlockedApp(name: string, matchNames: readonly string[]): boolean {
      const merged = mergeApps(config.desktop.blockedApps, { name, matchNames });
      if (!merged) return false;
      updateDesktop({ blockedApps: merged });
      return true;
    },
    addBlockedMobileApp(nameInput: string, packageInput: string): boolean {
      const name = normalizeDistractionsAppName(nameInput);
      const packageName = normalizeDistractionsMobilePackage(packageInput);
      if (!name || !packageName) return false;
      if (config.mobile.blockedApps.some((rule) => (
        rule.packageName.toLowerCase() === packageName.toLowerCase()
      ))) return true;
      updateMobile({
        blockedApps: [...config.mobile.blockedApps, { name, packageName, enabled: true }],
      });
      return true;
    },
    removeBlockedHost(host: string): void {
      update({ blockedHosts: removeHost(config.blockedHosts, host) });
    },
    removeExceptionHost(host: string): void {
      update({ exceptionHosts: removeHost(config.exceptionHosts, host) });
    },
    removeAllowedHost(host: string): void {
      update({ allowedHosts: removeHost(config.allowedHosts, host) });
    },
    removeBlockedApp(name: string): void {
      updateDesktop({ blockedApps: removeApp(config.desktop.blockedApps, name) });
    },
    removeBlockedMobileApp(packageName: string): void {
      const key = packageName.toLowerCase();
      updateMobile({
        blockedApps: config.mobile.blockedApps.filter((rule) => (
          rule.packageName.toLowerCase() !== key
        )),
      });
    },
    setBlockedHostEnabled(host: string, enabled: boolean): void {
      update({ blockedHosts: setHostEnabled(config.blockedHosts, host, enabled) });
    },
    setExceptionHostEnabled(host: string, enabled: boolean): void {
      update({ exceptionHosts: setHostEnabled(config.exceptionHosts, host, enabled) });
    },
    setAllowedHostEnabled(host: string, enabled: boolean): void {
      update({ allowedHosts: setHostEnabled(config.allowedHosts, host, enabled) });
    },
    setBlockedAppEnabled(name: string, enabled: boolean): void {
      updateDesktop({ blockedApps: setAppEnabled(config.desktop.blockedApps, name, enabled) });
    },
    setBlockedMobileAppEnabled(packageName: string, enabled: boolean): void {
      const key = packageName.toLowerCase();
      updateMobile({
        blockedApps: config.mobile.blockedApps.map((rule) => (
          rule.packageName.toLowerCase() === key ? { ...rule, enabled } : rule
        )),
      });
    },
    addUsageLimit(draft: DistractionsUsageLimitDraft): SaveDistractionsUsageLimitResult {
      const normalized = normalizeLimitDraft(draft);
      if (typeof normalized === "string") return normalized;
      const existingIds = new Set(config.limits.items.map((limit) => limit.id));
      let id = createUsageLimitId(normalized.name);
      while (existingIds.has(id)) {
        id = createUsageLimitId(normalized.name);
      }
      updateLimits({
        items: [
          ...config.limits.items,
          {
            id,
            enabled: true,
            ...normalized,
          },
        ],
      });
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
      updateLimits({ items });
      return "saved";
    },
    removeUsageLimit(id: string): void {
      updateLimits({ items: config.limits.items.filter((limit) => limit.id !== id) });
    },
    setUsageLimitEnabled(id: string, enabled: boolean): void {
      updateLimits({
        items: config.limits.items.map((limit) =>
          limit.id === id ? { ...limit, enabled } : limit
        ),
      });
    },
    reset(): void {
      persist({ ...DEFAULT_DISTRACTIONS_CONFIG });
    },
  };
}
