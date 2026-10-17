import { readFile, writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import path from "node:path";

const unicodeVersionFlag = "--unicode-version";
const committedVersionPattern = /^export const PROJECT_EMOJI_CATALOG_VERSION = "([^"]+)";$/mu;
const skinToneCodePoints = new Set(["1F3FB", "1F3FC", "1F3FD", "1F3FE", "1F3FF"]);
const skinToneNames = new Map([
  ["1F3FB", "light"],
  ["1F3FC", "medium-light"],
  ["1F3FD", "medium"],
  ["1F3FE", "medium-dark"],
  ["1F3FF", "dark"],
]);
const categoryByUnicodeGroup = new Map([
  ["Smileys & Emotion", "smileys"],
  ["People & Body", "people"],
  ["Animals & Nature", "nature"],
  ["Food & Drink", "food"],
  ["Travel & Places", "travel"],
  ["Activities", "activity"],
  ["Objects", "objects"],
  ["Symbols", "symbols"],
  ["Flags", "flags"],
]);
const categoryLabels = new Map([
  ["smileys", "Smileys"],
  ["people", "People"],
  ["nature", "Nature"],
  ["food", "Food"],
  ["activity", "Activity"],
  ["travel", "Travel"],
  ["objects", "Objects"],
  ["symbols", "Symbols"],
  ["flags", "Flags"],
]);
const categoryIcons = new Map([
  ["smileys", "smile"],
  ["people", "user-round"],
  ["nature", "leaf"],
  ["food", "utensils"],
  ["activity", "trophy"],
  ["travel", "plane"],
  ["objects", "package"],
  ["symbols", "shapes"],
  ["flags", "flag"],
]);

function emojiFromCodePoints(codePoints) {
  return codePoints.map((codePoint) => String.fromCodePoint(Number.parseInt(codePoint, 16))).join("");
}

function hasSkinTone(codePoints) {
  return codePoints.some((codePoint) => skinToneCodePoints.has(codePoint));
}

function stripSkinToneCodePoints(codePoints) {
  return codePoints.filter((codePoint) => !skinToneCodePoints.has(codePoint));
}

function sameSkinToneName(codePoints) {
  const tones = codePoints.filter((codePoint) => skinToneCodePoints.has(codePoint));
  if (tones.length === 0) return undefined;
  const firstTone = tones[0];
  if (!tones.every((tone) => tone === firstTone)) return undefined;
  return skinToneNames.get(firstTone);
}

function normalizeTerms(value) {
  return value
    .replace(/[&:]/g, " ")
    .replace(/[-_]/g, " ")
    .replace(/\s+/g, " ")
    .trim();
}

function tableCell(value) {
  return value.replace(/\t|\r?\n/gu, " ");
}

function parseEmojiTest(text) {
  let version = "unknown";
  let sourceDate = "unknown";
  let unicodeGroup = "";
  let unicodeSubgroup = "";
  const entries = [];
  const variants = new Map();

  for (const line of text.split(/\r?\n/u)) {
    const versionMatch = /^# Version:\s*(.+)$/u.exec(line);
    if (versionMatch) {
      version = versionMatch[1].trim();
      continue;
    }

    const dateMatch = /^# Date:\s*(.+)$/u.exec(line);
    if (dateMatch) {
      sourceDate = dateMatch[1].trim();
      continue;
    }

    const groupMatch = /^# group:\s*(.+)$/u.exec(line);
    if (groupMatch) {
      unicodeGroup = groupMatch[1].trim();
      continue;
    }

    const subgroupMatch = /^# subgroup:\s*(.+)$/u.exec(line);
    if (subgroupMatch) {
      unicodeSubgroup = subgroupMatch[1].trim();
      continue;
    }

    if (!line || line.startsWith("#")) continue;

    const match = /^([0-9A-F ]+)\s*;\s*([a-z-]+)\s*#\s*(\S+)\s+E[0-9.]+\s+(.+)$/u.exec(line);
    if (!match) continue;

    const [, rawCodePoints, status, rawEmoji, name] = match;
    if (status !== "fully-qualified") continue;

    const category = categoryByUnicodeGroup.get(unicodeGroup);
    if (!category) continue;

    const codePoints = rawCodePoints.trim().split(/\s+/u);
    const emoji = emojiFromCodePoints(codePoints);
    if (emoji !== rawEmoji) {
      throw new Error(`Parsed emoji mismatch for ${rawCodePoints}: ${emoji} != ${rawEmoji}`);
    }

    if (hasSkinTone(codePoints)) {
      const tone = sameSkinToneName(codePoints);
      if (!tone) continue;
      const baseEmoji = emojiFromCodePoints(stripSkinToneCodePoints(codePoints));
      const existing = variants.get(baseEmoji) ?? {};
      existing[tone] = emoji;
      variants.set(baseEmoji, existing);
      continue;
    }

    const terms = normalizeTerms(`${emoji} ${name} ${unicodeGroup} ${unicodeSubgroup} ${category}`);
    entries.push({
      emoji,
      name,
      category,
      terms,
    });
  }

  const variantEntries = [...variants.entries()]
    .filter(([baseEmoji]) => entries.some((entry) => entry.emoji === baseEmoji))
    .sort((left, right) => left[0].localeCompare(right[0]));

  return {
    version,
    sourceDate,
    entries,
    variantEntries,
  };
}

/**
 * Returns the versioned Unicode emoji-test.txt URL.
 *
 * Since Unicode 17.0 the emoji data lives inside the versioned UCD tree.
 *
 * @param {string} version Unicode emoji version such as `17.0`.
 * @returns {string} Source URL for that exact version.
 */
function emojiTestUrl(version) {
  return `https://www.unicode.org/Public/${version}.0/emoji/emoji-test.txt`;
}

/**
 * Resolves the Unicode emoji version to generate from.
 *
 * The committed catalog pins the version so regeneration is reproducible.
 * Passing `--unicode-version <version>` is the explicit way to upgrade it.
 *
 * @param {readonly string[]} args Command line arguments after the script path.
 * @param {string} outputPath Path of the committed generated catalog.
 * @returns {Promise<string>} Version to download.
 */
async function resolveUnicodeVersion(args, outputPath) {
  const flagIndex = args.indexOf(unicodeVersionFlag);
  if (flagIndex !== -1) {
    const requested = args[flagIndex + 1];
    if (!requested || !/^\d+\.\d+$/u.test(requested)) {
      throw new Error(`${unicodeVersionFlag} requires a version such as 17.0`);
    }
    return requested;
  }

  const committed = await readFile(outputPath, "utf8").catch((error) => {
    if (error?.code === "ENOENT") return "";
    throw error;
  });
  const match = committedVersionPattern.exec(committed);
  if (!match) {
    throw new Error(`No committed emoji catalog version found; pass ${unicodeVersionFlag} <version>`);
  }
  return match[1];
}

function generatedFileContents(parsed, sourceUrl) {
  const categoryIds = [...categoryLabels.keys()];
  const categories = categoryIds.map((id) => ({
    id,
    label: categoryLabels.get(id),
    icon: categoryIcons.get(id),
  }));
  const entryRows = parsed.entries.map((entry) =>
    [
      tableCell(entry.emoji),
      tableCell(entry.category),
      tableCell(entry.name),
      tableCell(entry.terms),
    ].join("\t")
  );
  const variantRows = parsed.variantEntries.map(([baseEmoji, variants]) =>
    [
      tableCell(baseEmoji),
      tableCell(variants.light ?? ""),
      tableCell(variants["medium-light"] ?? ""),
      tableCell(variants.medium ?? ""),
      tableCell(variants["medium-dark"] ?? ""),
      tableCell(variants.dark ?? ""),
    ].join("\t")
  );

  return `// Generated by scripts/codegen/generate-project-emoji-catalog.mjs from Unicode emoji-test.txt.
// Source: ${sourceUrl}
// Unicode emoji version: ${parsed.version}
// Source date: ${parsed.sourceDate}

export const PROJECT_EMOJI_CATALOG_VERSION = ${JSON.stringify(parsed.version)};

export const PROJECT_EMOJI_CATEGORY_IDS = ${JSON.stringify(categoryIds, null, 2)} as const;

export type ProjectEmojiCategoryId = (typeof PROJECT_EMOJI_CATEGORY_IDS)[number];

export interface ProjectEmojiEntry {
  emoji: string;
  name: string;
  category: ProjectEmojiCategoryId;
  terms: string;
}

export interface ProjectEmojiCategory {
  id: ProjectEmojiCategoryId;
  label: string;
  icon: string;
}

export const PROJECT_EMOJI_CATEGORIES: readonly ProjectEmojiCategory[] = ${JSON.stringify(categories, null, 2)};

const PROJECT_EMOJI_ENTRY_ROWS: readonly string[] = ${JSON.stringify(entryRows, null, 2)};

function projectEmojiCategoryFromRow(value: string): ProjectEmojiCategoryId {
  const category = PROJECT_EMOJI_CATEGORY_IDS.find((categoryId) => categoryId === value);
  if (!category) {
    throw new Error(\`Unknown project emoji category: \${value}\`);
  }
  return category;
}

function projectEmojiEntryFromRow(row: string): ProjectEmojiEntry {
  const [emoji, category, name, terms] = row.split("\\t");
  if (!emoji || !category || !name || !terms) {
    throw new Error(\`Invalid project emoji catalog row: \${row}\`);
  }
  return {
    emoji,
    name,
    category: projectEmojiCategoryFromRow(category),
    terms,
  };
}

export const PROJECT_EMOJI_ENTRIES: readonly ProjectEmojiEntry[] = PROJECT_EMOJI_ENTRY_ROWS.map(projectEmojiEntryFromRow);

type ProjectEmojiSkinToneVariantMap = Partial<Record<"light" | "medium-light" | "medium" | "medium-dark" | "dark", string>>;

const PROJECT_EMOJI_SKIN_TONE_VARIANT_ROWS: readonly string[] = ${JSON.stringify(variantRows, null, 2)};

function projectEmojiSkinToneVariantFromRow(row: string): readonly [string, ProjectEmojiSkinToneVariantMap] {
  const [baseEmoji, light, mediumLight, medium, mediumDark, dark] = row.split("\\t");
  if (!baseEmoji) {
    throw new Error(\`Invalid project emoji skin tone row: \${row}\`);
  }
  return [
    baseEmoji,
    {
      ...(light ? { light } : {}),
      ...(mediumLight ? { "medium-light": mediumLight } : {}),
      ...(medium ? { medium } : {}),
      ...(mediumDark ? { "medium-dark": mediumDark } : {}),
      ...(dark ? { dark } : {}),
    },
  ];
}

const PROJECT_EMOJI_SKIN_TONE_VARIANT_ENTRIES = PROJECT_EMOJI_SKIN_TONE_VARIANT_ROWS.map(projectEmojiSkinToneVariantFromRow);

export const PROJECT_EMOJI_SKIN_TONE_BASES: readonly string[] = PROJECT_EMOJI_SKIN_TONE_VARIANT_ENTRIES.map(([baseEmoji]) => baseEmoji);

export const PROJECT_EMOJI_SKIN_TONE_VARIANTS: Record<string, ProjectEmojiSkinToneVariantMap> = Object.fromEntries(PROJECT_EMOJI_SKIN_TONE_VARIANT_ENTRIES);
`;
}

async function main() {
  const scriptDir = path.dirname(fileURLToPath(import.meta.url));
  const outputPath = path.resolve(scriptDir, "../../src/lib/projects/icons/emoji-catalog.generated.ts");
  const version = await resolveUnicodeVersion(process.argv.slice(2), outputPath);
  const sourceUrl = emojiTestUrl(version);
  const response = await fetch(sourceUrl);
  if (!response.ok) {
    throw new Error(`Failed to download emoji data: ${response.status} ${response.statusText}`);
  }
  const text = await response.text();
  const parsed = parseEmojiTest(text);
  if (parsed.version !== version) {
    throw new Error(`Requested Unicode emoji ${version} but the source declares ${parsed.version}`);
  }
  await writeFile(outputPath, generatedFileContents(parsed, sourceUrl), "utf8");
  console.log(`Wrote ${parsed.entries.length} emoji entries from Unicode emoji ${parsed.version}`);
}

await main();
