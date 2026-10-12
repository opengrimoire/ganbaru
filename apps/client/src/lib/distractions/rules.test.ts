import { describe, expect, it } from "vitest";
import {
  DEFAULT_DISTRACTIONS_CONFIG,
  evaluateDistractionsUrl,
  isProtectedDistractionsDesktopAppName,
  normalizeDistractionsAppName,
  normalizeDistractionsConfig,
  normalizeDistractionsHost,
  normalizeDistractionsMobilePackage,
  parseDistractionsHosts,
  type DistractionsAppRule,
  type DistractionsCategoryId,
  type DistractionsConfig,
  type DistractionsHostRule,
  type DistractionsUsageLimit,
} from "./rules";

function appRule(name: string, enabled = true): DistractionsAppRule {
  return { name, enabled, matchNames: [name] };
}

function hostRule(host: string, enabled = true): DistractionsHostRule {
  return { host, enabled };
}

function categoryRule(id: DistractionsCategoryId, enabled = true) {
  return { id, enabled };
}

function disabledCategoryRules(): DistractionsConfig["blockedCategories"] {
  return DEFAULT_DISTRACTIONS_CONFIG.blockedCategories.map((rule) => ({
    ...rule,
    enabled: false,
  }));
}

function categoryRulesWith(
  id: DistractionsCategoryId,
  enabled = true,
): DistractionsConfig["blockedCategories"] {
  return disabledCategoryRules().map((rule) => ({
    ...rule,
    enabled: rule.id === id ? enabled : rule.enabled,
  }));
}

function config(partial: Partial<DistractionsConfig>): DistractionsConfig {
  return {
    ...DEFAULT_DISTRACTIONS_CONFIG,
    blockedCategories: disabledCategoryRules(),
    customCategoryStacks: [],
    blockedHosts: [],
    exceptionHosts: [],
    allowedHosts: [],
    limits: { enabled: true, items: [] },
    ...partial,
  };
}

function limit(partial: Partial<DistractionsUsageLimit>): DistractionsUsageLimit {
  return {
    id: "limit-1",
    name: "Daily limit",
    enabled: true,
    minutesPerDay: 30,
    entries: [{
      id: "entry-1",
      name: null,
      websiteHost: "youtube.com",
      mobileAppName: null,
      desktopAppName: null,
      desktopAppMatchNames: [],
    }],
    ...partial,
  };
}

describe("normalizeDistractionsHost", () => {
  it("accepts copied URLs and lowercases hosts", () => {
    expect(normalizeDistractionsHost("https://WWW.Reddit.com/r/all?x=1")).toBe("www.reddit.com");
  });

  it("accepts wildcard-style host input by storing the base host", () => {
    expect(normalizeDistractionsHost("*.youtube.com")).toBe("youtube.com");
  });

  it("rejects unsafe or unsupported host rules", () => {
    expect(normalizeDistractionsHost("")).toBeNull();
    expect(normalizeDistractionsHost("*")).toBeNull();
    expect(normalizeDistractionsHost("https://user@example.com")).toBeNull();
  });
});

describe("parseDistractionsHosts", () => {
  it("deduplicates normalized hosts while keeping first-seen order", () => {
    expect(parseDistractionsHosts("reddit.com, https://reddit.com/r/all\nnews.ycombinator.com")).toEqual([
      "reddit.com",
      "news.ycombinator.com",
    ]);
  });
});

describe("normalizeDistractionsAppName", () => {
  it("keeps user-facing app names while trimming whitespace", () => {
    expect(normalizeDistractionsAppName("  Visual   Studio Code  ")).toBe("Visual Studio Code");
  });

  it("rejects empty app names", () => {
    expect(normalizeDistractionsAppName("   ")).toBeNull();
  });

  it("recognizes protected Ganbaru AI app names", () => {
    expect(isProtectedDistractionsDesktopAppName("Ganbaru AI")).toBe(true);
    expect(isProtectedDistractionsDesktopAppName("ganbaru-ai")).toBe(true);
    expect(isProtectedDistractionsDesktopAppName("org.opengrimoire.ganbaruai")).toBe(true);
    expect(isProtectedDistractionsDesktopAppName("org.opengrimoire.ganbaruai.dev")).toBe(true);
    expect(isProtectedDistractionsDesktopAppName("Steam")).toBe(false);
  });

  it("recognizes protected system app and runtime names", () => {
    expect(isProtectedDistractionsDesktopAppName("Terminal")).toBe(true);
    expect(isProtectedDistractionsDesktopAppName("System Monitor")).toBe(true);
    expect(isProtectedDistractionsDesktopAppName("gnome-shell")).toBe(true);
    expect(isProtectedDistractionsDesktopAppName("python3.12")).toBe(true);
    expect(isProtectedDistractionsDesktopAppName("explorer.exe")).toBe(true);
    expect(isProtectedDistractionsDesktopAppName("Discord")).toBe(false);
  });
});

describe("normalizeDistractionsMobilePackage", () => {
  it("accepts stable Android package identifiers", () => {
    expect(normalizeDistractionsMobilePackage("  com.example.video_app  ")).toBe(
      "com.example.video_app",
    );
  });

  it("rejects labels, single segments, and malformed identifiers", () => {
    expect(normalizeDistractionsMobilePackage("YouTube")).toBeNull();
    expect(normalizeDistractionsMobilePackage("1com.example.video")).toBeNull();
    expect(normalizeDistractionsMobilePackage("com.example-video")).toBeNull();
  });
});

describe("normalizeDistractionsConfig", () => {
  it("defaults news to disabled and other built-in categories to enabled", () => {
    const normalized = normalizeDistractionsConfig(null);
    expect(normalized).toEqual(DEFAULT_DISTRACTIONS_CONFIG);
    expect(normalized.blockedCategories.find((rule) => rule.id === "news")).toEqual(
      categoryRule("news", false),
    );
    const otherCategories = normalized.blockedCategories.filter((rule) => rule.id !== "news");
    expect(otherCategories.every((rule) => rule.enabled)).toBe(true);
  });

  it("normalizes malformed config without throwing", () => {
    expect(normalizeDistractionsConfig({
      enabled: true,
      blockedHosts: ["Reddit.com", "*", "youtube.com"],
      allowedHosts: "docs.example.com",
    })).toEqual({
      mode: "blacklist",
      enabled: true,
      blockDuringFocus: true,
      blockDuringShortBreaks: true,
      blockDuringLongBreaks: true,
      pauseDuringFocusPause: true,
      blockedCategories: DEFAULT_DISTRACTIONS_CONFIG.blockedCategories,
      customCategoryStacks: [],
      blockedHosts: [],
      exceptionHosts: [],
      allowedHosts: [],
      mobile: DEFAULT_DISTRACTIONS_CONFIG.mobile,
      desktop: DEFAULT_DISTRACTIONS_CONFIG.desktop,
      limits: DEFAULT_DISTRACTIONS_CONFIG.limits,
    });
  });

  it("drops scalar rule entries from predecessor config shapes", () => {
    const normalized = normalizeDistractionsConfig({
      blockedCategories: ["news"],
      blockedHosts: ["reddit.com"],
      exceptionHosts: ["youtube.com"],
      allowedHosts: ["github.com"],
      desktop: { blockedApps: ["Steam"] },
    });

    expect(normalized.blockedCategories.find((rule) => rule.id === "news")).toEqual(
      categoryRule("news", false),
    );
    expect(normalized.blockedHosts).toEqual([]);
    expect(normalized.exceptionHosts).toEqual([]);
    expect(normalized.allowedHosts).toEqual([]);
    expect(normalized.desktop.blockedApps).toEqual([]);
  });

  it("normalizes disabled host rules without dropping them", () => {
    expect(normalizeDistractionsConfig({
      blockedHosts: [
        { host: "Reddit.com", enabled: false },
        { host: "https://youtube.com/watch?v=1" },
      ],
    })).toMatchObject({
      blockedHosts: [hostRule("reddit.com", false), hostRule("youtube.com")],
    });
  });

  it("keeps allowed hosts scoped to whitelist mode", () => {
    expect(normalizeDistractionsConfig({
      allowedHosts: [hostRule("music.youtube.com")],
    })).toMatchObject({
      mode: "blacklist",
      exceptionHosts: [],
      allowedHosts: [hostRule("music.youtube.com")],
    });
  });

  it("keeps whitelist allowed hosts separate from blacklist exceptions", () => {
    expect(normalizeDistractionsConfig({
      mode: "whitelist",
      exceptionHosts: [hostRule("music.youtube.com")],
      allowedHosts: [hostRule("github.com")],
    })).toMatchObject({
      mode: "whitelist",
      exceptionHosts: [hostRule("music.youtube.com")],
      allowedHosts: [hostRule("github.com")],
    });
  });

  it("normalizes focus blocking independently from global enablement", () => {
    expect(normalizeDistractionsConfig({
      enabled: true,
      blockDuringFocus: false,
      blockDuringShortBreaks: true,
    })).toMatchObject({
      enabled: true,
      blockDuringFocus: false,
      blockDuringShortBreaks: true,
    });
  });

  it("normalizes built-in categories and custom category customCategories", () => {
    const normalized = normalizeDistractionsConfig({
      blockedCategories: [
        categoryRule("social-media"),
        { id: "streaming", enabled: false },
        { id: "news", enabled: true },
        { id: "unknown", enabled: true },
      ],
      customCategoryStacks: [
        {
          id: "research-traps",
          name: "  Research traps  ",
          enabled: false,
          hosts: [
            hostRule("news.ycombinator.com"),
            { host: "https://reddit.com/r/programming", enabled: true },
            { host: "*", enabled: true },
          ],
        },
        {
          id: "bad id",
          name: "Invalid",
          hosts: [hostRule("example.com")],
        },
      ],
    });
    expect(normalized.blockedCategories.find((rule) => rule.id === "social-media")).toEqual(
      categoryRule("social-media"),
    );
    expect(normalized.blockedCategories.find((rule) => rule.id === "streaming")).toEqual(
      categoryRule("streaming", false),
    );
    expect(normalized.blockedCategories.find((rule) => rule.id === "news")).toEqual(
      categoryRule("news"),
    );
    expect(normalized.customCategoryStacks).toEqual([
      {
        id: "research-traps",
        name: "Research traps",
        enabled: false,
        hosts: [hostRule("news.ycombinator.com"), hostRule("reddit.com")],
      },
    ]);
  });

  it("normalizes desktop app rules as a blocklist separate from browser rules", () => {
    expect(normalizeDistractionsConfig({
      desktop: {
        enabled: false,
        blockDuringFocus: false,
        blockedApps: [
          { name: "Steam" },
          { name: "Ganbaru AI" },
          { name: " steam ", enabled: false },
          { name: "Discord", enabled: false },
          { name: "Calculator", matchNames: ["gnome-calculator"] },
          { name: "Terminal", matchNames: ["gnome-terminal"] },
        ],
      },
    })).toMatchObject({
      desktop: {
        enabled: false,
        blockDuringFocus: false,
        blockDuringShortBreaks: true,
        blockDuringLongBreaks: true,
        blockedApps: [
          appRule("Steam"),
          appRule("Discord", false),
        ],
      },
    });
  });

  it("normalizes mobile app rules by stable package identity", () => {
    expect(normalizeDistractionsConfig({
      mobile: {
        enabled: false,
        blockDuringFocus: false,
        blockedApps: [
          { name: " YouTube ", packageName: "com.google.android.youtube" },
          { name: "Duplicate", packageName: "COM.GOOGLE.ANDROID.YOUTUBE", enabled: false },
          { name: "Invalid", packageName: "YouTube" },
        ],
      },
    })).toMatchObject({
      mobile: {
        enabled: false,
        blockDuringFocus: false,
        blockDuringShortBreaks: true,
        blockDuringLongBreaks: true,
        pauseDuringFocusPause: true,
        blockedApps: [{
          name: "YouTube",
          packageName: "com.google.android.youtube",
          enabled: true,
        }],
      },
    });
  });

  it("normalizes valid daily usage limits", () => {
    const normalized = normalizeDistractionsConfig({
      limits: {
        enabled: true,
        items: [
          {
            id: "youtube",
            name: "  YouTube  ",
            enabled: false,
            minutesPerDay: 45.8,
            entries: [
              {
                id: "youtube-main",
                name: "  YouTube main  ",
                websiteHost: "https://youtube.com/watch?v=1",
                desktopAppName: "FreeTube",
                desktopAppMatchNames: [],
                mobileAppName: "YouTube",
                mobileAppPackage: "com.google.android.youtube",
              },
            ],
          },
        ],
      },
    });

    expect(normalized.limits.items).toEqual([
      {
        id: "youtube",
        name: "YouTube",
        enabled: false,
        minutesPerDay: 45,
        entries: [
          {
            id: "youtube-main",
            name: "YouTube main",
            websiteHost: "youtube.com",
            mobileAppName: "YouTube",
            mobileAppPackage: "com.google.android.youtube",
            desktopAppName: "FreeTube",
            desktopAppMatchNames: ["FreeTube"],
          },
        ],
      },
    ]);
  });

  it("rejects invalid daily usage limits", () => {
    const normalized = normalizeDistractionsConfig({
      limits: {
        items: [
          {
            id: "bad-minutes",
            name: "Bad",
            minutesPerDay: 0,
            entries: [{ id: "entry-1", websiteHost: "youtube.com" }],
          },
          {
            id: "bad-name",
            name: "   ",
            minutesPerDay: 30,
            entries: [{ id: "entry-1", websiteHost: "youtube.com" }],
          },
          {
            id: "duplicate-sources",
            name: "Duplicate sources",
            minutesPerDay: 30,
            entries: [
              { id: "entry-1", websiteHost: "youtube.com" },
              { id: "entry-2", websiteHost: "https://youtube.com/watch?v=1" },
            ],
          },
          {
            id: "protected-app",
            name: "Protected app",
            minutesPerDay: 30,
            entries: [{ id: "entry-1", desktopAppName: "Terminal" }],
          },
        ],
      },
    });

    expect(normalized.limits.items).toEqual([]);
  });
});

describe("evaluateDistractionsUrl", () => {
  it("blocks subdomains of blocked hosts", () => {
    const decision = evaluateDistractionsUrl("https://old.reddit.com/r/all", config({
      mode: "blacklist",
      blockedHosts: [hostRule("reddit.com")],
    }));
    expect(decision).toEqual({
      blocked: true,
      host: "old.reddit.com",
      matchedRule: "blocked host: reddit.com",
    });
  });

  it("lets exceptions override blocked parent domains", () => {
    const decision = evaluateDistractionsUrl("https://music.youtube.com/playlist?list=1", config({
      mode: "blacklist",
      blockedHosts: [hostRule("youtube.com")],
      exceptionHosts: [hostRule("music.youtube.com")],
    }));
    expect(decision.blocked).toBe(false);
    expect(decision.matchedRule).toBe("exception: music.youtube.com");
  });

  it("blocks domains outside whitelist mode", () => {
    const decision = evaluateDistractionsUrl("https://reddit.com/r/all", config({
      mode: "whitelist",
      allowedHosts: [hostRule("github.com")],
    }));
    expect(decision.blocked).toBe(true);
    expect(decision.matchedRule).toBe("not in whitelist");
  });

  it("allows domains inside whitelist mode", () => {
    const decision = evaluateDistractionsUrl("https://docs.github.com/en", config({
      mode: "whitelist",
      allowedHosts: [hostRule("github.com")],
    }));
    expect(decision.blocked).toBe(false);
    expect(decision.matchedRule).toBe("whitelist: github.com");
  });

  it("ignores disabled blacklist rules", () => {
    const decision = evaluateDistractionsUrl("https://reddit.com/r/all", config({
      mode: "blacklist",
      blockedHosts: [hostRule("reddit.com", false)],
    }));
    expect(decision.blocked).toBe(false);
    expect(decision.matchedRule).toBeNull();
  });

  it("ignores disabled whitelist rules", () => {
    const decision = evaluateDistractionsUrl("https://docs.github.com/en", config({
      mode: "whitelist",
      allowedHosts: [hostRule("github.com", false)],
    }));
    expect(decision.blocked).toBe(true);
    expect(decision.matchedRule).toBe("not in whitelist");
  });

  it("blocks enabled built-in categories in blacklist mode", () => {
    const decision = evaluateDistractionsUrl("https://old.reddit.com/r/all", config({
      mode: "blacklist",
      blockedCategories: categoryRulesWith("social-media"),
    }));
    expect(decision.blocked).toBe(true);
    expect(decision.matchedRule).toBe("category: Social media");
  });

  it("blocks streaming category keyword matches in domains", () => {
    const decision = evaluateDistractionsUrl("https://watch-anime.example/episode/1", config({
      mode: "blacklist",
      blockedCategories: categoryRulesWith("streaming"),
    }));
    expect(decision.blocked).toBe(true);
    expect(decision.matchedRule).toBe("category: Streaming");
  });

  it.each([
    ["https://local-news.example/story", "news", "category: News"],
    ["https://live-scores.example/game", "sports", "category: Sports"],
    ["https://online-casino.example/table", "gambling", "category: Gambling"],
    ["https://mini-game.example/play", "gaming", "category: Gaming"],
    ["https://shopee.example/deals", "shopping", "category: Shopping"],
    ["https://best-hookup.example/profile", "dating", "category: Dating"],
    ["https://crypto-watch.example/chart", "trading", "category: Trading"],
  ] satisfies Array<[string, DistractionsCategoryId, string]>)(
    "blocks built-in category keyword matches in domains for %s",
    (url, categoryId, matchedRule) => {
      const decision = evaluateDistractionsUrl(url, config({
        mode: "blacklist",
        blockedCategories: categoryRulesWith(categoryId),
      }));
      expect(decision.blocked).toBe(true);
      expect(decision.matchedRule).toBe(matchedRule);
    },
  );

  it("blocks porn category keyword matches in domains", () => {
    const decision = evaluateDistractionsUrl("https://example-porn-site.test/watch", config({
      mode: "blacklist",
      blockedCategories: categoryRulesWith("porn"),
    }));
    expect(decision.blocked).toBe(true);
    expect(decision.matchedRule).toBe("category: Porn");
  });

  it("blocks porn category keyword matches in reddit subreddit names", () => {
    const decision = evaluateDistractionsUrl("https://old.reddit.com/r/gwstories/comments/123/title", config({
      mode: "blacklist",
      blockedCategories: categoryRulesWith("porn"),
    }));
    expect(decision.blocked).toBe(true);
    expect(decision.matchedRule).toBe("category: Porn");
  });

  it("ignores reddit post titles for porn category keyword matching", () => {
    const decision = evaluateDistractionsUrl("https://reddit.com/r/productivity/comments/123/nsfw_post_title", config({
      mode: "blacklist",
      blockedCategories: categoryRulesWith("porn"),
    }));
    expect(decision.blocked).toBe(false);
    expect(decision.matchedRule).toBeNull();
  });

  it("blocks enabled custom category customCategories in blacklist mode", () => {
    const decision = evaluateDistractionsUrl("https://news.ycombinator.com/item?id=1", config({
      mode: "blacklist",
      customCategoryStacks: [
        {
          id: "research-traps",
          name: "Research traps",
          enabled: true,
          hosts: [hostRule("news.ycombinator.com")],
        },
      ],
    }));
    expect(decision.blocked).toBe(true);
    expect(decision.matchedRule).toBe("custom stack: Research traps");
  });

  it("ignores blacklist categories while in whitelist mode", () => {
    const decision = evaluateDistractionsUrl("https://old.reddit.com/r/all", config({
      mode: "whitelist",
      blockedCategories: categoryRulesWith("social-media"),
      allowedHosts: [hostRule("reddit.com")],
    }));
    expect(decision.blocked).toBe(false);
    expect(decision.matchedRule).toBe("whitelist: reddit.com");
  });
});
