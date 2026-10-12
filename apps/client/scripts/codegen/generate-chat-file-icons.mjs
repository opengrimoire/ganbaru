import { mkdir, readFile, rm, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const scriptDirectory = path.dirname(fileURLToPath(import.meta.url));
const clientDirectory = path.resolve(scriptDirectory, "..", "..");
const manifestPath = path.join(scriptDirectory, "chat-file-icons.manifest.json");
const sourceArgumentIndex = process.argv.indexOf("--source");
const sourceDirectory = sourceArgumentIndex >= 0 ? process.argv[sourceArgumentIndex + 1] : undefined;

if (sourceArgumentIndex >= 0 && !sourceDirectory) {
  throw new Error("The --source option requires a vscode-icons checkout path");
}

const manifest = parseManifest(JSON.parse(await readFile(manifestPath, "utf8")));
const iconEntries = Object.entries(manifest.icons).sort(([left], [right]) => left.localeCompare(right));
const generatedIcons = await mapWithConcurrency(iconEntries, 8, async ([iconId, sourceFile]) => {
  const source = await readUpstreamFile(sourceFile);
  return { iconId, svg: createStandaloneIcon(iconId, source) };
});
const license = await readUpstreamFile("LICENSE", false);

const outputDirectory = path.join(clientDirectory, "static", "file-icons");
const iconOutputDirectory = path.join(outputDirectory, "icons");
await mkdir(outputDirectory, { recursive: true });
await rm(iconOutputDirectory, { recursive: true, force: true });
await mkdir(iconOutputDirectory, { recursive: true });
await rm(path.join(outputDirectory, "vscode-icons.svg"), { force: true });
await mapWithConcurrency(generatedIcons, 16, ({ iconId, svg }) => (
  writeFile(path.join(iconOutputDirectory, `${iconId}.svg`), svg, "utf8")
));
await writeFile(path.join(outputDirectory, "vscode-icons-LICENSE.txt"), license, "utf8");
const generatedModulePath = path.join(clientDirectory, "src", "lib", "chat", "workspace", "file-icons.generated.ts");
const iconUrlPrefix = path.relative(path.dirname(generatedModulePath), iconOutputDirectory).split(path.sep).join("/");
await writeFile(
  generatedModulePath,
  generatedTypeScript(iconEntries.map(([iconId]) => iconId), manifest, iconUrlPrefix),
  "utf8",
);

/**
 * Validates the checked-in icon source manifest.
 *
 * @param {unknown} value Parsed JSON value.
 * @returns {{ upstreamRepository: string; upstreamRevision: string; icons: Record<string, string> }} Validated manifest.
 */
function parseManifest(value) {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new Error("The Chat file icon manifest must be an object");
  }
  const record = /** @type {Record<string, unknown>} */ (value);
  if (typeof record.upstreamRepository !== "string" || !/^https:\/\/github\.com\//.test(record.upstreamRepository)) {
    throw new Error("The Chat file icon upstream repository must be a GitHub HTTPS URL");
  }
  if (typeof record.upstreamRevision !== "string" || !/^[a-f0-9]{40}$/.test(record.upstreamRevision)) {
    throw new Error("The Chat file icon upstream revision must be a full Git commit");
  }
  if (typeof record.icons !== "object" || record.icons === null || Array.isArray(record.icons)) {
    throw new Error("The Chat file icon manifest must provide an icon map");
  }
  const icons = /** @type {Record<string, unknown>} */ (record.icons);
  for (const [iconId, sourceFile] of Object.entries(icons)) {
    if (!/^[a-z0-9-]+$/.test(iconId)) throw new Error(`Invalid Chat file icon ID: ${iconId}`);
    if (typeof sourceFile !== "string" || !/^(?:default_file|file_type_[a-z0-9_]+)\.svg$/.test(sourceFile)) {
      throw new Error(`Invalid vscode-icons source file for ${iconId}`);
    }
  }
  return {
    upstreamRepository: record.upstreamRepository,
    upstreamRevision: record.upstreamRevision,
    icons: /** @type {Record<string, string>} */ (icons),
  };
}

/**
 * Maps values with bounded concurrency to avoid overwhelming the pinned source host.
 *
 * @template T, U
 * @param {readonly T[]} values Input values.
 * @param {number} concurrency Maximum active operations.
 * @param {(value: T, index: number) => Promise<U>} mapper Asynchronous mapper.
 * @returns {Promise<U[]>} Results in input order.
 */
async function mapWithConcurrency(values, concurrency, mapper) {
  const results = /** @type {U[]} */ (new Array(values.length));
  let nextIndex = 0;
  async function worker() {
    while (nextIndex < values.length) {
      const index = nextIndex;
      nextIndex += 1;
      const value = values[index];
      if (value !== undefined) results[index] = await mapper(value, index);
    }
  }
  await Promise.all(Array.from({ length: Math.min(concurrency, values.length) }, () => worker()));
  return results;
}

/**
 * Loads one pinned upstream file from a local checkout or GitHub raw content.
 *
 * @param {string} relativePath Upstream repository-relative path under icons, or the license file.
 * @param {boolean} [insideIcons=true] Whether the file lives under the icons directory.
 * @returns {Promise<string>} UTF-8 source text.
 */
async function readUpstreamFile(relativePath, insideIcons = true) {
  const repositoryPath = insideIcons ? path.join("icons", relativePath) : relativePath;
  if (sourceDirectory) {
    return readFile(path.join(sourceDirectory, repositoryPath), "utf8");
  }
  const repositorySlug = new URL(manifest.upstreamRepository).pathname.replace(/^\//, "");
  const url = `https://raw.githubusercontent.com/${repositorySlug}/${manifest.upstreamRevision}/${repositoryPath}`;
  const response = await fetch(url, { redirect: "error" });
  if (!response.ok) throw new Error(`Could not download ${relativePath}: HTTP ${response.status}`);
  return response.text();
}

/**
 * Converts a trusted pinned SVG document into an isolated local image.
 *
 * @param {string} iconId Stable Ganbaru icon identifier.
 * @param {string} source Upstream SVG source.
 * @returns {string} Minified standalone SVG document.
 */
function createStandaloneIcon(iconId, source) {
  const sanitizedSource = source.replace(/<script\s*\/>/gi, "");
  if (/<(?:script|foreignObject|iframe|object|embed)\b|\son[a-z]+\s*=/i.test(sanitizedSource)) {
    throw new Error(`Unsafe SVG element or event attribute in ${iconId}`);
  }
  if (/\b(?:href|xlink:href)\s*=\s*["'](?!#)/i.test(sanitizedSource)) {
    throw new Error(`External SVG reference in ${iconId}`);
  }
  const match = sanitizedSource.match(/<svg\b([^>]*)>([\s\S]*?)<\/svg>/i);
  if (!match) throw new Error(`Invalid SVG source for ${iconId}`);
  const width = match[1].match(/\bwidth\s*=\s*["']([\d.]+)["']/i)?.[1];
  const height = match[1].match(/\bheight\s*=\s*["']([\d.]+)["']/i)?.[1];
  const viewBox = match[1].match(/\bviewBox\s*=\s*["']([^"']+)["']/i)?.[1]
    ?? (width && height ? `0 0 ${width} ${height}` : undefined);
  if (!viewBox || !/^-?[\d.]+(?:\s+-?[\d.]+){3}$/.test(viewBox)) {
    throw new Error(`Missing or invalid SVG viewBox for ${iconId}`);
  }
  const prefix = `chat-file-${iconId}-internal-`;
  let body = match[2]
    .replace(/<!--([\s\S]*?)-->/g, "")
    .replace(/<title\b[^>]*>[\s\S]*?<\/title>/gi, "")
    .replace(/<link\b[^>]*\/?\s*>/gi, "");
  const internalIds = [...body.matchAll(/\bid\s*=\s*["']([^"']+)["']/g)].map((entry) => entry[1]);
  for (const internalId of internalIds) {
    const escaped = escapeRegularExpression(internalId);
    body = body
      .replace(new RegExp(`(\\bid\\s*=\\s*["'])${escaped}(["'])`, "g"), `$1${prefix}${internalId}$2`)
      .replace(new RegExp(`url\\(#${escaped}\\)`, "g"), `url(#${prefix}${internalId})`)
      .replace(new RegExp(`((?:href|xlink:href)\\s*=\\s*["'])#${escaped}(["'])`, "g"), `$1#${prefix}${internalId}$2`);
  }
  body = body.replace(/>\s+</g, "><").trim();
  return `<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" viewBox="${viewBox}">${body}</svg>\n`;
}

/**
 * Escapes text for use in a regular expression.
 *
 * @param {string} value Literal source value.
 * @returns {string} Escaped expression text.
 */
function escapeRegularExpression(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

/**
 * Creates the small typed runtime contract for the generated local images.
 *
 * @param {string[]} iconIds Generated icon identifiers.
 * @param {{ upstreamRepository: string; upstreamRevision: string }} sourceManifest Upstream metadata.
 * @param {string} iconUrlPrefix Icon directory path relative to the generated module, with forward slashes.
 * @returns {string} TypeScript module source.
 */
function generatedTypeScript(iconIds, sourceManifest, iconUrlPrefix) {
  const ids = iconIds.map((iconId) => `  ${JSON.stringify(iconId)},`).join("\n");
  const urls = iconIds
    .map((iconId) => `  ${JSON.stringify(iconId)}: new URL(${JSON.stringify(`${iconUrlPrefix}/${iconId}.svg?no-inline`)}, import.meta.url).href,`)
    .join("\n");
  return `// Generated by scripts/codegen/generate-chat-file-icons.mjs. Do not edit manually.\n\n`
    + `export const CHAT_FILE_ICON_SOURCE = ${JSON.stringify(sourceManifest.upstreamRepository)};\n`
    + `export const CHAT_FILE_ICON_SOURCE_REVISION = ${JSON.stringify(sourceManifest.upstreamRevision)};\n`
    + `export const CHAT_FILE_ICON_IDS = [\n${ids}\n] as const;\n\n`
    + `export type ChatFileIconId = (typeof CHAT_FILE_ICON_IDS)[number];\n\n`
    + `export const CHAT_FILE_ICON_URLS = {\n${urls}\n} as const satisfies Record<ChatFileIconId, string>;\n\n`
    + `/**\n * Returns the local image URL for a generated file icon.\n *\n * @param iconId Generated file icon identifier.\n * @returns Same-origin standalone SVG URL.\n */\n`
    + `export function chatFileIconUrl(iconId: ChatFileIconId): string {\n`
    + `  return CHAT_FILE_ICON_URLS[iconId];\n}\n`;
}
