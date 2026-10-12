import { readFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { assertBaselinePathsExist, baselineEntries, clientFileSystem } from "./baseline-paths.mjs";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const clientDir = path.resolve(scriptDir, "..", "..");
const metadataPath = path.join(
  clientDir,
  ".bundle-contracts",
  "android",
  "first-use-bundle-metadata.json",
);
const baselinePath = path.join(scriptDir, "baselines", "android.json");
const requiredDestinationNames = [
  "shell",
  "calendar",
  "projects",
  "notes",
  "quick-notes",
  "music",
  "chat",
];

/** Read and parse one JSON file with a path-aware error. */
async function readJson(filePath) {
  try {
    return JSON.parse(await readFile(filePath, "utf8"));
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    throw new Error(`read ${path.relative(clientDir, filePath)}: ${message}`);
  }
}

/** Require a plain object at a contract boundary. */
function requireObject(value, label) {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new Error(`${label} must be an object`);
  }
  return value;
}

/** Require a non-empty string at a contract boundary. */
function requireString(value, label) {
  if (typeof value !== "string" || value.length === 0) {
    throw new Error(`${label} must be a non-empty string`);
  }
  return value;
}

/** Require a positive safe integer at a contract boundary. */
function requirePositiveInteger(value, label) {
  if (!Number.isSafeInteger(value) || value < 1) {
    throw new Error(`${label} must be a positive integer`);
  }
  return value;
}

/** Require an array of non-empty strings. */
function requireStringArray(value, label) {
  if (!Array.isArray(value) || value.some((item) => typeof item !== "string" || item.length === 0)) {
    throw new Error(`${label} must be an array of non-empty strings`);
  }
  return value;
}

/** Reject duplicate strings because they hide accidental baseline drift. */
function requireUniqueStrings(values, label) {
  const unique = new Set(values);
  if (unique.size !== values.length) throw new Error(`${label} must not contain duplicates`);
  return values;
}

/** Validate the committed Android bundle limits and exclusions. */
function readBaseline(value) {
  const root = requireObject(value, "Android bundle baseline");
  if (root.schemaVersion !== 2 || !Array.isArray(root.destinations)) {
    throw new Error("Android bundle baseline has an unsupported schema");
  }

  const destinations = root.destinations.map((value, index) => {
    const destination = requireObject(value, `Android destination ${index}`);
    const name = requireString(destination.name, `Android destination ${index} name`);
    const roots = requireUniqueStrings(
      requireStringArray(destination.roots, `Android destination ${name} roots`),
      `Android destination ${name} roots`,
    );
    if (roots.length === 0) throw new Error(`Android destination ${name} roots must not be empty`);
    return {
      name,
      roots,
      maxSourceModules: requirePositiveInteger(
        destination.maxSourceModules,
        `Android destination ${name} maxSourceModules`,
      ),
    };
  });

  const destinationNames = requireUniqueStrings(
    destinations.map((destination) => destination.name),
    "Android destination names",
  );
  for (const name of requiredDestinationNames) {
    if (!destinationNames.includes(name)) {
      throw new Error(`Android bundle baseline is missing destination: ${name}`);
    }
  }

  return {
    destinations,
    maxTotalSourceModules: requirePositiveInteger(
      root.maxTotalSourceModules,
      "Android bundle maxTotalSourceModules",
    ),
    requiredMobileModules: requireUniqueStrings(
      requireStringArray(root.requiredMobileModules, "Android requiredMobileModules"),
      "Android requiredMobileModules",
    ),
    forbiddenModules: requireUniqueStrings(
      requireStringArray(root.forbiddenModules, "Android forbiddenModules"),
      "Android forbiddenModules",
    ),
    forbiddenModuleSubstrings: requireUniqueStrings(
      requireStringArray(
        root.forbiddenModuleSubstrings,
        "Android forbiddenModuleSubstrings",
      ),
      "Android forbiddenModuleSubstrings",
    ),
  };
}

/** Validate chunk graph metadata emitted by the production Vite build. */
function readChunks(value) {
  const root = requireObject(value, "Android bundle metadata");
  if (root.schemaVersion !== 1 || !Array.isArray(root.chunks)) {
    throw new Error("Android bundle metadata has an unsupported schema");
  }
  if (root.chunks.length === 0) throw new Error("Android bundle metadata has no chunks");

  const chunks = root.chunks.map((value, index) => {
    const chunk = requireObject(value, `Android bundle chunk ${index}`);
    const facadeModuleId = chunk.facadeModuleId;
    if (facadeModuleId !== null && typeof facadeModuleId !== "string") {
      throw new Error(`Android bundle chunk ${index} facadeModuleId must be a string or null`);
    }
    return {
      fileName: requireString(chunk.fileName, `Android bundle chunk ${index} fileName`),
      facadeModuleId,
      imports: requireStringArray(chunk.imports, `Android bundle chunk ${index} imports`),
      modules: requireStringArray(chunk.modules, `Android bundle chunk ${index} modules`),
    };
  });
  requireUniqueStrings(chunks.map((chunk) => chunk.fileName), "Android chunk file names");
  return chunks;
}

/** Find chunks that contain or expose a destination root module. */
function chunksForRoot(chunks, rootModule) {
  return chunks.filter(
    (chunk) => chunk.facadeModuleId === rootModule || chunk.modules.includes(rootModule),
  );
}

/** Traverse transitive static imports from a set of emitted root chunks. */
function staticChunkClosure(rootChunkNames, chunksByFile, failures, destinationName) {
  const closure = new Set();
  const pending = [...rootChunkNames];
  while (pending.length > 0) {
    const fileName = pending.pop();
    if (closure.has(fileName)) continue;
    closure.add(fileName);
    const chunk = chunksByFile.get(fileName);
    if (!chunk) {
      failures.push(
        `Android ${destinationName} static closure references missing chunk: ${fileName}`,
      );
      continue;
    }
    pending.push(...chunk.imports);
  }
  return closure;
}

/** Collect normalized application source modules from emitted chunks. */
function sourceModulesForChunks(chunkNames, chunksByFile) {
  const sourceModules = new Set();
  for (const fileName of chunkNames) {
    const chunk = chunksByFile.get(fileName);
    if (!chunk) continue;
    for (const moduleId of chunk.modules) {
      if (moduleId.startsWith("src/")) sourceModules.add(moduleId);
    }
  }
  return sourceModules;
}

const baseline = readBaseline(await readJson(baselinePath));
assertBaselinePathsExist("Android bundle", [
  ...baseline.destinations.flatMap((destination) => (
    baselineEntries(`destination ${destination.name} roots`, destination.roots, "exact")
  )),
  ...baselineEntries("requiredMobileModules", baseline.requiredMobileModules, "exact"),
  ...baselineEntries("forbiddenModules", baseline.forbiddenModules, "exact"),
  ...baselineEntries("forbiddenModuleSubstrings", baseline.forbiddenModuleSubstrings, "pattern"),
], clientFileSystem(clientDir));
const chunks = readChunks(await readJson(metadataPath));
const chunksByFile = new Map(chunks.map((chunk) => [chunk.fileName, chunk]));
const modules = new Set(chunks.flatMap((chunk) => chunk.modules));
const sourceModules = new Set([...modules].filter((moduleId) => moduleId.startsWith("src/")));
const failures = [];
const destinationResults = {};

for (const destination of baseline.destinations) {
  const rootChunkNames = new Set();
  for (const rootModule of destination.roots) {
    const matchingChunks = chunksForRoot(chunks, rootModule);
    if (matchingChunks.length === 0) {
      failures.push(`Android ${destination.name} is missing root module: ${rootModule}`);
    }
    for (const chunk of matchingChunks) rootChunkNames.add(chunk.fileName);
  }

  const staticChunks = staticChunkClosure(
    rootChunkNames,
    chunksByFile,
    failures,
    destination.name,
  );
  const destinationSourceModules = sourceModulesForChunks(staticChunks, chunksByFile);
  if (destinationSourceModules.size > destination.maxSourceModules) {
    failures.push(
      `Android ${destination.name} static closure has ${destinationSourceModules.size} source modules, baseline allows ${destination.maxSourceModules}`,
    );
  }
  destinationResults[destination.name] = {
    roots: destination.roots,
    rootChunks: [...rootChunkNames].sort(),
    staticChunks: staticChunks.size,
    sourceModules: destinationSourceModules.size,
    maxSourceModules: destination.maxSourceModules,
  };
}

if (sourceModules.size > baseline.maxTotalSourceModules) {
  failures.push(
    `Android build has ${sourceModules.size} source modules, baseline allows ${baseline.maxTotalSourceModules}`,
  );
}

for (const moduleId of baseline.requiredMobileModules) {
  if (!modules.has(moduleId)) failures.push(`Android build is missing mobile module: ${moduleId}`);
}
for (const moduleId of baseline.forbiddenModules) {
  if (modules.has(moduleId)) failures.push(`Android build contains forbidden module: ${moduleId}`);
}
for (const substring of baseline.forbiddenModuleSubstrings) {
  for (const moduleId of modules) {
    if (moduleId.includes(substring)) {
      failures.push(`Android build contains forbidden module: ${moduleId}`);
    }
  }
}

if (failures.length > 0) {
  throw new Error(
    `Android bundle contract failed:\n${failures.map((failure) => `- ${failure}`).join("\n")}`,
  );
}

console.log(JSON.stringify({
  platform: "android",
  metadata: path.relative(clientDir, metadataPath),
  totalSourceModules: sourceModules.size,
  maxTotalSourceModules: baseline.maxTotalSourceModules,
  destinations: destinationResults,
  requiredMobileModules: baseline.requiredMobileModules.length,
  forbiddenRules: baseline.forbiddenModules.length + baseline.forbiddenModuleSubstrings.length,
}, null, 2));
