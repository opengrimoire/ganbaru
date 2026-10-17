import { readFile, writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import path from "node:path";

const scriptDirectory = path.dirname(fileURLToPath(import.meta.url));
const clientDirectory = path.resolve(scriptDirectory, "..", "..");
const lucidePackagePath = path.join(
  clientDirectory,
  "node_modules",
  "@lucide",
  "svelte",
  "package.json",
);
const catalogPath = path.join(
  clientDirectory,
  "src",
  "lib",
  "projects",
  "icons",
  "lucide-catalog.generated.ts",
);
const catalogEntryPattern = /^(\s*\{ slug: )("(?:[^"\\]|\\.)*")(, label: )("(?:[^"\\]|\\.)*")(, category: )("(?:[^"\\]|\\.)*")(, terms: )("(?:[^"\\]|\\.)*")(, iconNode: .*)$/u;
const generatedHeaderPattern = /^\/\/ Lucide English search tags generated from lucide-static@[^\n]+\n\/\/ Source: [^\n]+\n/u;
const appSuppliedTagsBySlug = new Map([
  ["sport-shoe", ["sneaker", "running", "exercise", "fitness", "footwear"]],
]);

function normalizedTerm(value) {
  return value.trim().toLowerCase().replace(/\s+/gu, " ");
}

function searchTerms(slug, label, tags) {
  return [...new Set([
    slug,
    slug.replaceAll("-", " "),
    label,
    ...tags,
  ].map(normalizedTerm).filter(Boolean))].join(" ");
}

function validateTags(value) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new Error("Lucide tags must be an object keyed by icon slug");
  }

  const result = new Map();
  for (const [slug, rawTags] of Object.entries(value)) {
    if (!Array.isArray(rawTags) || rawTags.length === 0) {
      throw new Error(`Lucide tags for ${slug} must be a non-empty array`);
    }
    const tags = rawTags.map((tag) => {
      if (typeof tag !== "string" || !tag.trim()) {
        throw new Error(`Lucide tag for ${slug} must be a non-empty string`);
      }
      return normalizedTerm(tag);
    });
    result.set(slug, tags);
  }
  return result;
}

const lucidePackage = JSON.parse(await readFile(lucidePackagePath, "utf8"));
const lucideVersion = lucidePackage.version;
if (typeof lucideVersion !== "string" || !/^\d+\.\d+\.\d+$/u.test(lucideVersion)) {
  throw new Error("Installed @lucide/svelte package has an invalid version");
}

const sourceUrl = `https://cdn.jsdelivr.net/npm/lucide-static@${lucideVersion}/tags.json`;
const response = await fetch(sourceUrl, { headers: { accept: "application/json" } });
if (!response.ok) {
  throw new Error(`Could not load Lucide ${lucideVersion} tags: HTTP ${response.status}`);
}
const tagsBySlug = validateTags(await response.json());
const source = await readFile(catalogPath, "utf8");
const missingTags = [];
let updatedEntryCount = 0;

const updatedCatalog = source
  .replace(generatedHeaderPattern, "")
  .split("\n")
  .map((line) => {
    const match = catalogEntryPattern.exec(line);
    if (!match) return line;

    const [, beforeSlug, slugLiteral, beforeLabel, labelLiteral, beforeCategory, categoryLiteral, beforeTerms, , afterTerms] = match;
    const slug = JSON.parse(slugLiteral);
    const label = JSON.parse(labelLiteral);
    const tags = tagsBySlug.get(slug) ?? appSuppliedTagsBySlug.get(slug);
    if (!tags) {
      missingTags.push(slug);
      return line;
    }

    updatedEntryCount += 1;
    return `${beforeSlug}${slugLiteral}${beforeLabel}${labelLiteral}${beforeCategory}${categoryLiteral}${beforeTerms}${JSON.stringify(searchTerms(slug, label, tags))}${afterTerms}`;
  })
  .join("\n");

if (updatedEntryCount === 0) {
  throw new Error("No Lucide catalog entries were found");
}
if (missingTags.length > 0) {
  throw new Error(`Lucide tags are missing for: ${missingTags.slice(0, 10).join(", ")}`);
}

const header = `// Lucide English search tags generated from lucide-static@${lucideVersion}.\n// Source: ${sourceUrl}\n`;
await writeFile(catalogPath, `${header}${updatedCatalog}`, "utf8");
process.stdout.write(`Updated English search tags for ${updatedEntryCount} Lucide icons.\n`);
