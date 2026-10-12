import rawDistractionsCategoryDefinitions from "./categories.json";
import { PALETTE_SIZE, type EventColor } from "$lib/calendar/types";

export type DistractionsMode = "blacklist" | "whitelist";

export const DISTRACTIONS_CATEGORY_IDS = [
  "social-media",
  "streaming",
  "news",
  "sports",
  "porn",
  "gambling",
  "gaming",
  "shopping",
  "dating",
  "trading",
] as const;

export type DistractionsCategoryId = (typeof DISTRACTIONS_CATEGORY_IDS)[number];

interface DistractionsCategoryDefinition {
  id: DistractionsCategoryId;
  label: string;
  hosts: readonly string[];
  domainKeywords?: readonly string[];
  redditSubredditKeywords?: readonly string[];
}

export const DISTRACTIONS_PROTECTED_DESKTOP_APP_NAMES = [
  "Ganbaru AI",
  "ganbaru-ai",
  "ganbaru-ai-dev",
  "Ganbaru AI Dev",
  "Ganbaru AI (dev)",
  "org.opengrimoire.ganbaruai",
  "org.opengrimoire.ganbaruai.dev",
  "Activity Monitor",
  "Advanced Network Configuration",
  "Calculator",
  "Characters",
  "Clocks",
  "Command Prompt",
  "Console",
  "Control Panel",
  "Disk Utility",
  "Disk Usage Analyzer",
  "Disks",
  "Event Viewer",
  "Extension Manager",
  "Extensions",
  "File Explorer",
  "Files",
  "Finder",
  "Fonts",
  "GDebi Package Installer",
  "GNOME System Monitor",
  "Help",
  "Htop",
  "IBus Preferences",
  "Input Method",
  "Keychain Access",
  "Language Support",
  "Logs",
  "Notepad",
  "Passwords and Keys",
  "Power Statistics",
  "PowerShell",
  "Settings",
  "Startup Applications",
  "System Monitor",
  "System Preferences",
  "System Settings",
  "Task Manager",
  "Terminal",
  "Text Editor",
  "UXTerm",
  "Windows Explorer",
  "Windows PowerShell",
  "XTerm",
] as const;

export const DISTRACTIONS_PROTECTED_DESKTOP_PROCESS_NAMES = [
  "Activity Monitor",
  "bash",
  "cmd",
  "cmd.exe",
  "conhost.exe",
  "ControlCenter",
  "csrss.exe",
  "dash",
  "dbus-broker",
  "dbus-daemon",
  "dllhost.exe",
  "Dock",
  "dwm.exe",
  "electron",
  "explorer.exe",
  "Finder",
  "fish",
  "flatpak",
  "gnome-control-center",
  "gnome-keyring-daemon",
  "gnome-shell",
  "gnome-terminal",
  "gnome-terminal-server",
  "ibus-daemon",
  "java",
  "javaw",
  "javaw.exe",
  "kitty",
  "konsole",
  "kwin_wayland",
  "kwin_x11",
  "launchd",
  "loginwindow",
  "mmc.exe",
  "mutter",
  "node",
  "plasmashell",
  "PowerShell",
  "powershell.exe",
  "pwsh",
  "pwsh.exe",
  "python",
  "python3",
  "python3.11",
  "python3.12",
  "pythonw.exe",
  "regedit.exe",
  "rundll32.exe",
  "services.exe",
  "sh",
  "ShellExperienceHost.exe",
  "sihost.exe",
  "snap",
  "StartMenuExperienceHost.exe",
  "svchost.exe",
  "System Settings",
  "SystemUIServer",
  "taskmgr.exe",
  "Terminal",
  "wezterm",
  "winlogon.exe",
  "WindowsTerminal.exe",
  "WindowServer",
  "wt.exe",
  "wscript.exe",
  "xdg-desktop-portal",
  "xdg-desktop-portal-gnome",
  "xdg-desktop-portal-gtk",
  "Xorg",
  "XTerm",
  "Xwayland",
  "zsh",
] as const;

const DISTRACTIONS_CATEGORY_ID_SET = new Set<string>(DISTRACTIONS_CATEGORY_IDS);

function isKnownDistractionsCategoryId(value: string): value is DistractionsCategoryId {
  return DISTRACTIONS_CATEGORY_ID_SET.has(value);
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function parseCategoryStringList(
  value: unknown,
  categoryId: string,
  field: string,
): readonly string[] {
  if (value === undefined) return [];
  if (!Array.isArray(value)) {
    throw new Error(`Invalid distraction category ${categoryId}.${field}`);
  }
  const strings: string[] = [];
  for (const item of value) {
    if (typeof item !== "string" || item.trim() === "") {
      throw new Error(`Invalid distraction category ${categoryId}.${field}`);
    }
    strings.push(item);
  }
  return strings;
}

function loadDistractionsCategoryDefinitions(value: unknown): readonly DistractionsCategoryDefinition[] {
  if (!Array.isArray(value)) {
    throw new Error("Distractions category definitions must be an array");
  }
  const definitions: DistractionsCategoryDefinition[] = [];
  const seenIds = new Set<string>();
  for (const item of value) {
    if (!isRecord(item)) {
      throw new Error("Distractions category definitions must contain objects");
    }
    const id = item.id;
    const label = item.label;
    if (typeof id !== "string" || !isKnownDistractionsCategoryId(id)) {
      throw new Error("Distractions category definition has an unsupported id");
    }
    if (seenIds.has(id)) {
      throw new Error(`Duplicate distraction category id: ${id}`);
    }
    if (typeof label !== "string" || label.trim() === "") {
      throw new Error(`Distractions category ${id} has an invalid label`);
    }
    seenIds.add(id);
    definitions.push({
      id,
      label,
      hosts: parseCategoryStringList(item.hosts, id, "hosts"),
      domainKeywords: parseCategoryStringList(item.domainKeywords, id, "domainKeywords"),
      redditSubredditKeywords: parseCategoryStringList(
        item.redditSubredditKeywords,
        id,
        "redditSubredditKeywords",
      ),
    });
  }
  for (const id of DISTRACTIONS_CATEGORY_IDS) {
    if (!seenIds.has(id)) {
      throw new Error(`Missing distraction category id: ${id}`);
    }
  }
  return definitions;
}

export const DISTRACTIONS_CATEGORY_DEFINITIONS = loadDistractionsCategoryDefinitions(
  rawDistractionsCategoryDefinitions,
);

export interface DistractionsHostRule {
  host: string;
  enabled: boolean;
}

export interface DistractionsAppRule {
  name: string;
  enabled: boolean;
  matchNames: string[];
}

export interface DistractionsMobileAppRule {
  name: string;
  packageName: string;
  enabled: boolean;
}

export interface DistractionsCategoryRule {
  id: DistractionsCategoryId;
  enabled: boolean;
}

export interface DistractionsCustomCategory {
  id: string;
  name: string;
  enabled: boolean;
  hosts: DistractionsHostRule[];
}

export interface DistractionsDesktopConfig {
  enabled: boolean;
  blockDuringFocus: boolean;
  blockDuringShortBreaks: boolean;
  blockDuringLongBreaks: boolean;
  pauseDuringFocusPause: boolean;
  blockedApps: DistractionsAppRule[];
}

export interface DistractionsMobileConfig {
  enabled: boolean;
  blockDuringFocus: boolean;
  blockDuringShortBreaks: boolean;
  blockDuringLongBreaks: boolean;
  pauseDuringFocusPause: boolean;
  blockedApps: DistractionsMobileAppRule[];
}

export interface DistractionsLimitEntry {
  id: string;
  name: string | null;
  color?: EventColor | null;
  websiteHost: string | null;
  mobileAppName: string | null;
  mobileAppPackage?: string | null;
  desktopAppName: string | null;
  desktopAppMatchNames: string[];
}

export interface DistractionsUsageLimit {
  id: string;
  name: string;
  enabled: boolean;
  minutesPerDay: number | null;
  minutesPerWeek?: number | null;
  entries: DistractionsLimitEntry[];
}

export interface DistractionsUsageLimitsConfig {
  enabled: boolean;
  items: DistractionsUsageLimit[];
}

export interface DistractionsConfig {
  mode: DistractionsMode;
  enabled: boolean;
  blockDuringFocus: boolean;
  blockDuringShortBreaks: boolean;
  blockDuringLongBreaks: boolean;
  pauseDuringFocusPause: boolean;
  blockedCategories: DistractionsCategoryRule[];
  customCategoryStacks: DistractionsCustomCategory[];
  blockedHosts: DistractionsHostRule[];
  exceptionHosts: DistractionsHostRule[];
  allowedHosts: DistractionsHostRule[];
  mobile: DistractionsMobileConfig;
  desktop: DistractionsDesktopConfig;
  limits: DistractionsUsageLimitsConfig;
}

export interface DistractionsDecision {
  blocked: boolean;
  host: string | null;
  matchedRule: string | null;
}

export type DistractionsLimitPeriod = "day" | "week";

export interface DistractionsLimitTotal {
  limitId: string;
  period?: DistractionsLimitPeriod;
  windowStartLocalDate?: string;
  windowEndLocalDate?: string;
  usedSeconds: number;
  limitSeconds: number;
  remainingSeconds: number;
  exhausted: boolean;
}

export interface DistractionsDailyLimitEntryTotal {
  entryId: string;
  usedSeconds: number;
}

const MAX_DAILY_LIMIT_MINUTES = 24 * 60;
const MAX_WEEKLY_LIMIT_MINUTES = 7 * 24 * 60;

function defaultCategoryRules(): DistractionsCategoryRule[] {
  return DISTRACTIONS_CATEGORY_DEFINITIONS.map((category) => ({
    id: category.id,
    enabled: category.id !== "news",
  }));
}

function defaultDesktopConfig(): DistractionsDesktopConfig {
  return {
    enabled: true,
    blockDuringFocus: true,
    blockDuringShortBreaks: true,
    blockDuringLongBreaks: true,
    pauseDuringFocusPause: true,
    blockedApps: [],
  };
}

function defaultMobileConfig(): DistractionsMobileConfig {
  return {
    enabled: true,
    blockDuringFocus: true,
    blockDuringShortBreaks: true,
    blockDuringLongBreaks: true,
    pauseDuringFocusPause: true,
    blockedApps: [],
  };
}

function defaultUsageLimitsConfig(): DistractionsUsageLimitsConfig {
  return {
    enabled: true,
    items: [],
  };
}

function protectedDesktopAppKeys(): Set<string> {
  const keys = new Set<string>();
  for (const name of DISTRACTIONS_PROTECTED_DESKTOP_APP_NAMES) {
    keys.add(appRuleKey(name));
  }
  for (const name of DISTRACTIONS_PROTECTED_DESKTOP_PROCESS_NAMES) {
    keys.add(appRuleKey(name));
  }
  return keys;
}

export const DEFAULT_DISTRACTIONS_CONFIG: DistractionsConfig = Object.freeze({
  mode: "blacklist",
  enabled: true,
  blockDuringFocus: true,
  blockDuringShortBreaks: true,
  blockDuringLongBreaks: true,
  pauseDuringFocusPause: true,
  blockedCategories: defaultCategoryRules(),
  customCategoryStacks: [],
  blockedHosts: [],
  exceptionHosts: [],
  allowedHosts: [],
  mobile: defaultMobileConfig(),
  desktop: defaultDesktopConfig(),
  limits: defaultUsageLimitsConfig(),
});

export function isDistractionsCategoryId(value: string): value is DistractionsCategoryId {
  return isKnownDistractionsCategoryId(value);
}

export function getDistractionsCategoryDefinition(
  id: DistractionsCategoryId,
): DistractionsCategoryDefinition | undefined {
  return DISTRACTIONS_CATEGORY_DEFINITIONS.find((category) => category.id === id);
}

function stripUrlParts(input: string): string {
  const trimmed = input.trim();
  if (trimmed.length === 0) return "";
  try {
    const withScheme = /^[a-z][a-z0-9+.-]*:\/\//i.test(trimmed)
      ? trimmed
      : `https://${trimmed}`;
    return new URL(withScheme).hostname;
  } catch {
    return trimmed.split(/[/?#]/, 1)[0] ?? "";
  }
}

/**
 * Normalize user-entered host rules into lowercase domain names.
 *
 * @param input - A domain, host, or URL copied from the browser.
 * @returns The normalized host, or null when the input is not a valid host rule.
 */
export function normalizeDistractionsHost(input: string): string | null {
  if (input.includes("@")) return null;
  const withoutProtocol = stripUrlParts(input).toLowerCase();
  const withoutWildcard = withoutProtocol.startsWith("*.") ? withoutProtocol.slice(2) : withoutProtocol;
  const withoutPort = withoutWildcard.replace(/:\d+$/, "");
  const host = withoutPort.replace(/\.+$/, "");
  if (host.length === 0) return null;
  if (host.includes("*") || host.includes(" ") || host.includes("@")) return null;
  if (host === "localhost") return host;
  if (/^\d{1,3}(?:\.\d{1,3}){3}$/.test(host)) return host;
  if (!/^[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?(?:\.[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?)+$/.test(host)) {
    return null;
  }
  return host;
}

/**
 * Parse a textarea-style list of host rules.
 *
 * @param input - Hosts separated by newlines, commas, semicolons, or spaces.
 * @returns Deduplicated normalized hosts in first-seen order.
 */
export function parseDistractionsHosts(input: string): string[] {
  const seen = new Set<string>();
  const hosts: string[] = [];
  for (const part of input.split(/[\s,;]+/)) {
    const host = normalizeDistractionsHost(part);
    if (!host || seen.has(host)) continue;
    seen.add(host);
    hosts.push(host);
  }
  return hosts;
}

/**
 * Normalize user-entered desktop app names.
 *
 * @param input - A visible app name or executable name entered by the user.
 * @returns The normalized app name, or null when the input is empty.
 */
export function normalizeDistractionsAppName(input: string): string | null {
  const name = input.trim().replace(/\s+/g, " ").slice(0, 80);
  if (name.length === 0) return null;
  if (/[\u0000-\u001f]/.test(name)) return null;
  return name;
}

/** Normalize a stable Android application package identifier. */
export function normalizeDistractionsMobilePackage(input: string): string | null {
  const packageName = input.trim();
  if (packageName.length === 0 || packageName.length > 255) return null;
  if (!/^[A-Za-z][A-Za-z0-9_]*(?:\.[A-Za-z][A-Za-z0-9_]*)+$/.test(packageName)) return null;
  return packageName;
}

export function isProtectedDistractionsDesktopAppName(input: string): boolean {
  const name = normalizeDistractionsAppName(input);
  return name ? protectedDesktopAppKeys().has(appRuleKey(name)) : false;
}

export function normalizeDistractionsDesktopAppMatchNames(
  displayName: string,
  matchNames: readonly string[] = [],
): string[] {
  const seen = new Set<string>();
  const normalized: string[] = [];
  for (const rawName of [displayName, ...matchNames]) {
    const name = normalizeDistractionsAppName(rawName);
    if (!name || isProtectedDistractionsDesktopAppName(name)) continue;
    const key = appRuleKey(name);
    if (seen.has(key)) continue;
    seen.add(key);
    normalized.push(name);
  }
  return normalized;
}

function normalizeHostRuleValue(value: unknown): DistractionsHostRule | null {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return null;
  const record = value as Record<string, unknown>;
  if (typeof record.host !== "string") return null;
  const host = normalizeDistractionsHost(record.host);
  if (!host) return null;
  return {
    host,
    enabled: record.enabled !== false,
  };
}

function normalizeHostRules(value: unknown): DistractionsHostRule[] {
  if (!Array.isArray(value)) return [];
  const seen = new Set<string>();
  const rules: DistractionsHostRule[] = [];
  for (const item of value) {
    const rule = normalizeHostRuleValue(item);
    if (!rule || seen.has(rule.host)) continue;
    seen.add(rule.host);
    rules.push(rule);
  }
  return rules;
}

function appRuleKey(name: string): string {
  return name.toLowerCase();
}

function normalizeAppRuleValue(value: unknown): DistractionsAppRule | null {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return null;
  const record = value as Record<string, unknown>;
  if (typeof record.name !== "string") return null;
  const name = normalizeDistractionsAppName(record.name);
  if (!name) return null;
  const matchNames = normalizeAppRuleMatchNames(name, record.matchNames);
  return {
    name,
    enabled: record.enabled !== false,
    matchNames,
  };
}

function normalizeAppRuleMatchNames(name: string, value: unknown): string[] {
  const seen = new Set<string>();
  const matchNames: string[] = [];
  const candidates = Array.isArray(value) ? [name, ...value] : [name];
  for (const candidate of candidates) {
    if (typeof candidate !== "string") continue;
    const matchName = normalizeDistractionsAppName(candidate);
    if (!matchName) continue;
    const key = appRuleKey(matchName);
    if (protectedDesktopAppKeys().has(key) || seen.has(key)) continue;
    seen.add(key);
    matchNames.push(matchName);
  }
  return matchNames.length > 0 ? matchNames : [name];
}

function normalizeAppRules(value: unknown, includeProtectedApps = false): DistractionsAppRule[] {
  if (!Array.isArray(value)) return [];
  const seen = new Set<string>();
  const rules: DistractionsAppRule[] = [];
  for (const item of value) {
    const rule = normalizeAppRuleValue(item);
    if (!rule) continue;
    const key = appRuleKey(rule.name);
    if (!includeProtectedApps && protectedDesktopAppKeys().has(key)) continue;
    if (seen.has(key)) continue;
    seen.add(key);
    rules.push(rule);
  }
  return rules;
}

function normalizeCategoryRuleValue(value: unknown): DistractionsCategoryRule | null {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return null;
  const record = value as Record<string, unknown>;
  if (typeof record.id !== "string" || !isDistractionsCategoryId(record.id)) return null;
  return {
    id: record.id,
    enabled: record.enabled === true,
  };
}

function normalizeCategoryRules(value: unknown): DistractionsCategoryRule[] {
  const rules = defaultCategoryRules();
  if (!Array.isArray(value)) return rules;
  const byId = new Map<DistractionsCategoryId, DistractionsCategoryRule>(
    rules.map((rule) => [rule.id, rule]),
  );
  for (const item of value) {
    const rule = normalizeCategoryRuleValue(item);
    if (!rule) continue;
    byId.set(rule.id, rule);
  }
  return rules.map((rule) => byId.get(rule.id) ?? rule);
}

function normalizeCustomCategoryId(value: unknown): string | null {
  if (typeof value !== "string") return null;
  const id = value.trim();
  if (!/^[a-z0-9][a-z0-9_-]{0,79}$/i.test(id)) return null;
  return id;
}

export function normalizeDistractionsCustomCategoryName(input: string): string | null {
  const name = input.trim().replace(/\s+/g, " ").slice(0, 60);
  return name.length > 0 ? name : null;
}

function normalizeCustomCategoryValue(value: unknown): DistractionsCustomCategory | null {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return null;
  const record = value as Record<string, unknown>;
  const id = normalizeCustomCategoryId(record.id);
  const name = typeof record.name === "string"
    ? normalizeDistractionsCustomCategoryName(record.name)
    : null;
  const hosts = normalizeHostRules(record.hosts);
  if (!id || !name || hosts.length === 0) return null;
  return {
    id,
    name,
    enabled: record.enabled !== false,
    hosts,
  };
}

function normalizeCustomCategories(value: unknown): DistractionsCustomCategory[] {
  if (!Array.isArray(value)) return [];
  const seenIds = new Set<string>();
  const customCategories: DistractionsCustomCategory[] = [];
  for (const item of value) {
    const customCategory = normalizeCustomCategoryValue(item);
    if (!customCategory || seenIds.has(customCategory.id)) continue;
    seenIds.add(customCategory.id);
    customCategories.push(customCategory);
  }
  return customCategories;
}

function normalizeUsageLimitId(value: unknown): string | null {
  if (typeof value !== "string") return null;
  const id = value.trim();
  if (!/^[a-z0-9][a-z0-9_-]{0,79}$/i.test(id)) return null;
  return id;
}

export function normalizeDistractionsUsageLimitName(input: string): string | null {
  const name = input.trim().replace(/\s+/g, " ").slice(0, 80);
  return name.length > 0 ? name : null;
}

export function normalizeDistractionsLimitEntryName(input: string): string | null {
  const name = input.trim().replace(/\s+/g, " ").slice(0, 80);
  return name.length > 0 ? name : null;
}

function normalizeLimitMinutes(value: unknown): number | null {
  if (typeof value !== "number" || !Number.isFinite(value)) return null;
  const minutes = Math.trunc(value);
  if (minutes < 1 || minutes > MAX_DAILY_LIMIT_MINUTES) return null;
  return minutes;
}

function normalizeOptionalLimitMinutes(value: unknown, maxMinutes: number): number | null {
  if (value === null || value === undefined) return null;
  if (typeof value !== "number" || !Number.isFinite(value)) return null;
  const minutes = Math.trunc(value);
  if (minutes < 1 || minutes > maxMinutes) return null;
  return minutes;
}

function normalizeLimitEntryId(value: unknown): string | null {
  if (typeof value !== "string") return null;
  const id = value.trim();
  if (!/^[a-z0-9][a-z0-9_-]{0,79}$/i.test(id)) return null;
  return id;
}

export function normalizeDistractionsLimitEntryColor(value: unknown): EventColor | null {
  if (typeof value !== "number" || !Number.isInteger(value)) return null;
  if (value < 0 || value >= PALETTE_SIZE) return null;
  return value;
}

function normalizeLimitEntryValue(value: unknown): DistractionsLimitEntry | null {
  if (!isRecord(value)) return null;
  const id = normalizeLimitEntryId(value.id);
  if (!id) return null;
  const name = typeof value.name === "string"
    ? normalizeDistractionsLimitEntryName(value.name)
    : null;
  const color = normalizeDistractionsLimitEntryColor(value.color);
  const websiteHost = typeof value.websiteHost === "string"
    ? normalizeDistractionsHost(value.websiteHost)
    : null;
  const mobileAppName = typeof value.mobileAppName === "string"
    ? normalizeDistractionsAppName(value.mobileAppName)
    : null;
  const mobileAppPackage = typeof value.mobileAppPackage === "string"
    ? normalizeDistractionsMobilePackage(value.mobileAppPackage)
    : null;
  const desktopAppName = typeof value.desktopAppName === "string"
    ? normalizeDistractionsAppName(value.desktopAppName)
    : null;
  const desktopAppMatchNames = desktopAppName
    ? normalizeDistractionsDesktopAppMatchNames(
      desktopAppName,
      Array.isArray(value.desktopAppMatchNames)
        ? value.desktopAppMatchNames.filter((item): item is string => typeof item === "string")
        : [],
    )
    : [];
  if (!websiteHost && !mobileAppName && !desktopAppName) return null;
  if (desktopAppName && isProtectedDistractionsDesktopAppName(desktopAppName)) return null;
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

export function distractionsLimitEntryKey(entry: DistractionsLimitEntry): string {
  return entry.id;
}

export function distractionsLimitEntrySourceKeys(entry: DistractionsLimitEntry): string[] {
  const keys: string[] = [];
  if (entry.websiteHost) keys.push(`website:${entry.websiteHost}`);
  if (entry.mobileAppPackage) {
    keys.push(`mobile-app:${entry.mobileAppPackage.toLowerCase()}`);
  } else if (entry.mobileAppName) {
    keys.push(`mobile-app:${entry.mobileAppName.toLowerCase()}`);
  }
  if (entry.desktopAppName) {
    const matchNames = entry.desktopAppMatchNames.length > 0
      ? entry.desktopAppMatchNames
      : [entry.desktopAppName];
    for (const matchName of matchNames) {
      keys.push(`desktop-app:${appRuleKey(matchName)}`);
    }
  }
  return keys;
}

function normalizeLimitEntries(value: unknown): DistractionsLimitEntry[] | null {
  if (!Array.isArray(value)) return null;
  const seen = new Set<string>();
  const entries: DistractionsLimitEntry[] = [];
  const seenIds = new Set<string>();
  for (const item of value) {
    const entry = normalizeLimitEntryValue(item);
    if (!entry || seenIds.has(entry.id)) return null;
    seenIds.add(entry.id);
    for (const key of distractionsLimitEntrySourceKeys(entry)) {
      if (seen.has(key)) return null;
      seen.add(key);
    }
    entries.push(entry);
  }
  return entries.length > 0 ? entries : null;
}

function normalizeUsageLimitValue(value: unknown): DistractionsUsageLimit | null {
  if (!isRecord(value)) return null;
  const id = normalizeUsageLimitId(value.id);
  const name = typeof value.name === "string"
    ? normalizeDistractionsUsageLimitName(value.name)
    : null;
  const minutesPerDay = normalizeLimitMinutes(value.minutesPerDay);
  const minutesPerWeek = normalizeOptionalLimitMinutes(value.minutesPerWeek, MAX_WEEKLY_LIMIT_MINUTES);
  const entries = normalizeLimitEntries(value.entries);
  if (!id || !name || !entries || (minutesPerDay === null && minutesPerWeek === null)) return null;
  const limit: DistractionsUsageLimit = {
    id,
    name,
    enabled: value.enabled !== false,
    minutesPerDay,
    entries,
  };
  return minutesPerWeek === null ? limit : { ...limit, minutesPerWeek };
}

function normalizeUsageLimitsConfig(value: unknown): DistractionsUsageLimitsConfig {
  if (!isRecord(value)) return defaultUsageLimitsConfig();
  const itemsValue = value.items;
  const items: DistractionsUsageLimit[] = [];
  const seenIds = new Set<string>();
  if (Array.isArray(itemsValue)) {
    for (const item of itemsValue) {
      const limit = normalizeUsageLimitValue(item);
      if (!limit || seenIds.has(limit.id)) continue;
      seenIds.add(limit.id);
      items.push(limit);
    }
  }
  return {
    enabled: value.enabled !== false,
    items,
  };
}

function normalizeMode(value: unknown): DistractionsMode {
  return value === "whitelist" || value === "blacklist"
    ? value
    : DEFAULT_DISTRACTIONS_CONFIG.mode;
}

function normalizeDesktopConfig(value: unknown): DistractionsDesktopConfig {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    return defaultDesktopConfig();
  }
  const record = value as Record<string, unknown>;
  return {
    enabled: typeof record.enabled === "boolean"
      ? record.enabled
      : true,
    blockDuringFocus: typeof record.blockDuringFocus === "boolean"
      ? record.blockDuringFocus
      : true,
    blockDuringShortBreaks: typeof record.blockDuringShortBreaks === "boolean"
      ? record.blockDuringShortBreaks
      : true,
    blockDuringLongBreaks: typeof record.blockDuringLongBreaks === "boolean"
      ? record.blockDuringLongBreaks
      : true,
    pauseDuringFocusPause: typeof record.pauseDuringFocusPause === "boolean"
      ? record.pauseDuringFocusPause
      : true,
    blockedApps: normalizeAppRules(record.blockedApps),
  };
}

function normalizeMobileAppRules(value: unknown): DistractionsMobileAppRule[] {
  if (!Array.isArray(value)) return [];
  const rules: DistractionsMobileAppRule[] = [];
  const seen = new Set<string>();
  for (const item of value) {
    if (!isRecord(item)) continue;
    const name = typeof item.name === "string" ? normalizeDistractionsAppName(item.name) : null;
    const packageName = typeof item.packageName === "string"
      ? normalizeDistractionsMobilePackage(item.packageName)
      : null;
    if (!name || !packageName) continue;
    const key = packageName.toLowerCase();
    if (seen.has(key)) continue;
    seen.add(key);
    rules.push({ name, packageName, enabled: item.enabled !== false });
  }
  return rules;
}

function normalizeMobileConfig(value: unknown): DistractionsMobileConfig {
  if (!isRecord(value)) return defaultMobileConfig();
  return {
    enabled: typeof value.enabled === "boolean" ? value.enabled : true,
    blockDuringFocus: typeof value.blockDuringFocus === "boolean" ? value.blockDuringFocus : true,
    blockDuringShortBreaks: typeof value.blockDuringShortBreaks === "boolean"
      ? value.blockDuringShortBreaks
      : true,
    blockDuringLongBreaks: typeof value.blockDuringLongBreaks === "boolean"
      ? value.blockDuringLongBreaks
      : true,
    pauseDuringFocusPause: typeof value.pauseDuringFocusPause === "boolean"
      ? value.pauseDuringFocusPause
      : true,
    blockedApps: normalizeMobileAppRules(value.blockedApps),
  };
}

export function normalizeDistractionsConfig(value: unknown): DistractionsConfig {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    return {
      ...DEFAULT_DISTRACTIONS_CONFIG,
      blockedCategories: defaultCategoryRules(),
      mobile: defaultMobileConfig(),
      desktop: defaultDesktopConfig(),
      limits: defaultUsageLimitsConfig(),
    };
  }
  const record = value as Record<string, unknown>;
  return {
    mode: normalizeMode(record.mode),
    enabled: typeof record.enabled === "boolean"
      ? record.enabled
      : DEFAULT_DISTRACTIONS_CONFIG.enabled,
    blockDuringFocus: typeof record.blockDuringFocus === "boolean"
      ? record.blockDuringFocus
      : DEFAULT_DISTRACTIONS_CONFIG.blockDuringFocus,
    blockDuringShortBreaks: typeof record.blockDuringShortBreaks === "boolean"
      ? record.blockDuringShortBreaks
      : DEFAULT_DISTRACTIONS_CONFIG.blockDuringShortBreaks,
    blockDuringLongBreaks: typeof record.blockDuringLongBreaks === "boolean"
      ? record.blockDuringLongBreaks
      : DEFAULT_DISTRACTIONS_CONFIG.blockDuringLongBreaks,
    pauseDuringFocusPause: typeof record.pauseDuringFocusPause === "boolean"
      ? record.pauseDuringFocusPause
      : DEFAULT_DISTRACTIONS_CONFIG.pauseDuringFocusPause,
    blockedCategories: normalizeCategoryRules(record.blockedCategories),
    customCategoryStacks: normalizeCustomCategories(record.customCategoryStacks),
    blockedHosts: normalizeHostRules(record.blockedHosts),
    exceptionHosts: normalizeHostRules(record.exceptionHosts),
    allowedHosts: normalizeHostRules(record.allowedHosts),
    mobile: normalizeMobileConfig(record.mobile),
    desktop: normalizeDesktopConfig(record.desktop),
    limits: normalizeUsageLimitsConfig(record.limits),
  };
}

export function distractionsHostMatchesRule(host: string, ruleHost: string): boolean {
  return host === ruleHost || host.endsWith(`.${ruleHost}`);
}

function redditSubredditFromUrl(parsedUrl: URL): string | null {
  if (!distractionsHostMatchesRule(parsedUrl.hostname.toLowerCase(), "reddit.com")) return null;
  const match = /^\/r\/([^/]+)/i.exec(parsedUrl.pathname);
  if (!match) return null;
  try {
    return decodeURIComponent(match[1]).toLowerCase();
  } catch {
    return match[1].toLowerCase();
  }
}

function categoryRuleMatchesUrl(
  parsedUrl: URL,
  host: string,
  categoryId: DistractionsCategoryId,
): boolean {
  const category = getDistractionsCategoryDefinition(categoryId);
  if (!category) return false;
  if (category.hosts.some((ruleHost) => distractionsHostMatchesRule(host, ruleHost))) {
    return true;
  }
  if (category.domainKeywords?.some((keyword) => host.includes(keyword))) {
    return true;
  }
  const subreddit = redditSubredditFromUrl(parsedUrl);
  return subreddit !== null
    && (category.redditSubredditKeywords?.some((keyword) => subreddit.includes(keyword)) ?? false);
}

/**
 * Evaluate a URL against the current host-only distraction rules.
 *
 * @param url - Browser URL to evaluate.
 * @param config - Distractions configuration.
 * @returns A decision that explains the matching host rule.
 */
export function evaluateDistractionsUrl(
  url: string,
  config: DistractionsConfig,
): DistractionsDecision {
  let parsed: URL;
  let host: string;
  try {
    parsed = new URL(url);
    if (parsed.protocol !== "http:" && parsed.protocol !== "https:") {
      return { blocked: false, host: null, matchedRule: null };
    }
    host = parsed.hostname.toLowerCase();
  } catch {
    return { blocked: false, host: null, matchedRule: null };
  }

  if (host === "localhost" || host === "127.0.0.1" || host === "::1" || host.endsWith(".localhost")) {
    return { blocked: false, host, matchedRule: "browser safety allowlist" };
  }

  if (config.mode === "whitelist") {
    for (const allowedRule of config.allowedHosts) {
      if (!allowedRule.enabled) continue;
      const allowedHost = allowedRule.host;
      if (distractionsHostMatchesRule(host, allowedHost)) {
        return { blocked: false, host, matchedRule: `whitelist: ${allowedHost}` };
      }
    }
    return { blocked: true, host, matchedRule: "not in whitelist" };
  }

  for (const exceptionRule of config.exceptionHosts) {
    if (!exceptionRule.enabled) continue;
    const exceptionHost = exceptionRule.host;
    if (distractionsHostMatchesRule(host, exceptionHost)) {
      return { blocked: false, host, matchedRule: `exception: ${exceptionHost}` };
    }
  }
  for (const blockedRule of config.blockedHosts) {
    if (!blockedRule.enabled) continue;
    const blockedHost = blockedRule.host;
    if (distractionsHostMatchesRule(host, blockedHost)) {
      return { blocked: true, host, matchedRule: `blocked host: ${blockedHost}` };
    }
  }
  for (const customCategory of config.customCategoryStacks) {
    if (!customCategory.enabled) continue;
    for (const hostRule of customCategory.hosts) {
      if (!hostRule.enabled) continue;
      if (distractionsHostMatchesRule(host, hostRule.host)) {
        return { blocked: true, host, matchedRule: `custom stack: ${customCategory.name}` };
      }
    }
  }
  for (const categoryRule of config.blockedCategories) {
    if (!categoryRule.enabled) continue;
    if (categoryRuleMatchesUrl(parsed, host, categoryRule.id)) {
      const category = getDistractionsCategoryDefinition(categoryRule.id);
      return { blocked: true, host, matchedRule: `category: ${category?.label ?? categoryRule.id}` };
    }
  }
  return { blocked: false, host, matchedRule: null };
}
