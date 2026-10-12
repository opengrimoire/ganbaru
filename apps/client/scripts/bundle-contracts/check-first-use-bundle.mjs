import { readFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { assertBaselinePathsExist, baselineEntries, clientFileSystem } from "./baseline-paths.mjs";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const clientDir = path.resolve(scriptDir, "..", "..");
const metadataPath = path.join(clientDir, "dist", "first-use-bundle-metadata.json");
const baselinePath = path.join(scriptDir, "baselines", "first-use.json");

async function readJson(filePath) {
  try {
    return JSON.parse(await readFile(filePath, "utf8"));
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    throw new Error(`read ${path.relative(clientDir, filePath)}: ${message}`);
  }
}

function requireObject(value, label) {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new Error(`${label} must be an object`);
  }
  return value;
}

function requireString(value, label) {
  if (typeof value !== "string" || value.length === 0) {
    throw new Error(`${label} must be a non-empty string`);
  }
  return value;
}

function requireStringArray(value, label) {
  if (!Array.isArray(value) || value.some((item) => typeof item !== "string")) {
    throw new Error(`${label} must be a string array`);
  }
  return value;
}

function readChunks(metadata) {
  const root = requireObject(metadata, "bundle metadata");
  if (root.schemaVersion !== 1 || !Array.isArray(root.chunks)) {
    throw new Error("bundle metadata has an unsupported schema");
  }
  return root.chunks.map((value, index) => {
    const chunk = requireObject(value, `bundle chunk ${index}`);
    return {
      fileName: requireString(chunk.fileName, `bundle chunk ${index} fileName`),
      isEntry: chunk.isEntry === true,
      imports: requireStringArray(chunk.imports, `bundle chunk ${index} imports`),
      dynamicImports: requireStringArray(
        chunk.dynamicImports,
        `bundle chunk ${index} dynamicImports`,
      ),
      modules: requireStringArray(chunk.modules, `bundle chunk ${index} modules`),
    };
  });
}

function readBaseline(value) {
  const root = requireObject(value, "first-use bundle baseline");
  if (root.schemaVersion !== 1 || !Array.isArray(root.routes)) {
    throw new Error("first-use bundle baseline has an unsupported schema");
  }
  const routes = root.routes.map((value, index) => {
    const route = requireObject(value, `baseline route ${index}`);
    const maxSourceModules = route.maxSourceModules;
    if (!Number.isSafeInteger(maxSourceModules) || maxSourceModules < 1) {
      throw new Error(`baseline route ${index} maxSourceModules must be a positive integer`);
    }
    return {
      name: requireString(route.name, `baseline route ${index} name`),
      module: requireString(route.module, `baseline route ${index} module`),
      maxSourceModules,
    };
  });
  return {
    routes,
    noVaultStartup: (() => {
      const contract = requireObject(root.noVaultStartup, "baseline noVaultStartup");
      if (!Array.isArray(contract.roots) || contract.roots.length === 0) {
        throw new Error("baseline noVaultStartup roots must be a non-empty array");
      }
      return {
        roots: contract.roots.map((value, index) => {
          const label = `baseline noVaultStartup root ${index}`;
          const root = requireObject(value, label);
          const loadedModules = requireStringArray(root.loadedModules, `${label} loadedModules`);
          if (loadedModules.length === 0) {
            throw new Error(`${label} loadedModules must not be empty`);
          }
          return { name: requireString(root.name, `${label} name`), loadedModules };
        }),
        forbiddenModules: requireStringArray(
          contract.forbiddenModules,
          "baseline noVaultStartup forbiddenModules",
        ),
        forbiddenModuleSubstrings: requireStringArray(
          contract.forbiddenModuleSubstrings,
          "baseline noVaultStartup forbiddenModuleSubstrings",
        ),
      };
    })(),
    shell: (() => {
      const shell = requireObject(root.shell, "baseline shell");
      return {
        loadedModules: requireStringArray(shell.loadedModules, "baseline shell loadedModules"),
        requiredModules: requireStringArray(shell.requiredModules, "baseline shell requiredModules"),
        forbiddenModules: requireStringArray(
          shell.forbiddenModules,
          "baseline shell forbiddenModules",
        ),
      };
    })(),
    chatShell: (() => {
      const contract = requireObject(root.chatShell, "baseline chatShell");
      return {
        loadedModules: requireStringArray(
          contract.loadedModules,
          "baseline chatShell loadedModules",
        ),
        requiredModules: requireStringArray(
          contract.requiredModules,
          "baseline chatShell requiredModules",
        ),
        forbiddenModules: requireStringArray(
          contract.forbiddenModules,
          "baseline chatShell forbiddenModules",
        ),
        forbiddenModuleSubstrings: requireStringArray(
          contract.forbiddenModuleSubstrings,
          "baseline chatShell forbiddenModuleSubstrings",
        ),
      };
    })(),
    editorRuntime: (() => {
      const contract = requireObject(root.editorRuntime, "baseline editorRuntime");
      return {
        module: requireString(contract.module, "baseline editorRuntime module"),
        requiredModuleSubstrings: requireStringArray(
          contract.requiredModuleSubstrings,
          "baseline editorRuntime requiredModuleSubstrings",
        ),
        requiredDynamicChunkPrefixes: requireStringArray(
          contract.requiredDynamicChunkPrefixes,
          "baseline editorRuntime requiredDynamicChunkPrefixes",
        ),
        forbiddenStaticChunkPrefixes: requireStringArray(
          contract.forbiddenStaticChunkPrefixes,
          "baseline editorRuntime forbiddenStaticChunkPrefixes",
        ),
      };
    })(),
    reviewRuntime: (() => {
      const contract = requireObject(root.reviewRuntime, "baseline reviewRuntime");
      const maxCatalogModulesPerChunk = contract.maxCatalogModulesPerChunk;
      if (!Number.isSafeInteger(maxCatalogModulesPerChunk) || maxCatalogModulesPerChunk < 1) {
        throw new Error(
          "baseline reviewRuntime maxCatalogModulesPerChunk must be a positive integer",
        );
      }
      return {
        module: requireString(contract.module, "baseline reviewRuntime module"),
        catalogModuleSubstrings: requireStringArray(
          contract.catalogModuleSubstrings,
          "baseline reviewRuntime catalogModuleSubstrings",
        ),
        requiredDynamicChunkPrefixes: requireStringArray(
          contract.requiredDynamicChunkPrefixes,
          "baseline reviewRuntime requiredDynamicChunkPrefixes",
        ),
        maxCatalogModulesPerChunk,
      };
    })(),
    projectsShell: (() => {
      const contract = requireObject(root.projectsShell, "baseline projectsShell");
      return {
        loadedModules: requireStringArray(
          contract.loadedModules,
          "baseline projectsShell loadedModules",
        ),
        requiredModules: requireStringArray(
          contract.requiredModules,
          "baseline projectsShell requiredModules",
        ),
        forbiddenModules: requireStringArray(
          contract.forbiddenModules,
          "baseline projectsShell forbiddenModules",
        ),
      };
    })(),
    projectsToolbar: (() => {
      const contract = requireObject(root.projectsToolbar, "baseline projectsToolbar");
      return {
        loadedModules: requireStringArray(
          contract.loadedModules,
          "baseline projectsToolbar loadedModules",
        ),
        requiredModules: requireStringArray(
          contract.requiredModules,
          "baseline projectsToolbar requiredModules",
        ),
        forbiddenModules: requireStringArray(
          contract.forbiddenModules,
          "baseline projectsToolbar forbiddenModules",
        ),
      };
    })(),
    notesShell: (() => {
      const contract = requireObject(root.notesShell, "baseline notesShell");
      return {
        loadedModules: requireStringArray(
          contract.loadedModules,
          "baseline notesShell loadedModules",
        ),
        requiredModules: requireStringArray(
          contract.requiredModules,
          "baseline notesShell requiredModules",
        ),
        forbiddenModules: requireStringArray(
          contract.forbiddenModules,
          "baseline notesShell forbiddenModules",
        ),
      };
    })(),
    notesParagraphEditor: (() => {
      const contract = requireObject(root.notesParagraphEditor, "baseline notesParagraphEditor");
      return {
        loadedModules: requireStringArray(
          contract.loadedModules,
          "baseline notesParagraphEditor loadedModules",
        ),
        forbiddenModules: requireStringArray(
          contract.forbiddenModules,
          "baseline notesParagraphEditor forbiddenModules",
        ),
      };
    })(),
    notesAdvancedModules: requireStringArray(
      root.notesAdvancedModules,
      "baseline notesAdvancedModules",
    ),
    notesDatabaseTable: (() => {
      const contract = requireObject(root.notesDatabaseTable, "baseline notesDatabaseTable");
      return {
        loadedModules: requireStringArray(
          contract.loadedModules,
          "baseline notesDatabaseTable loadedModules",
        ),
        forbiddenModules: requireStringArray(
          contract.forbiddenModules,
          "baseline notesDatabaseTable forbiddenModules",
        ),
      };
    })(),
    settingsAppearance: (() => {
      const contract = requireObject(root.settingsAppearance, "baseline settingsAppearance");
      return {
        loadedModules: requireStringArray(
          contract.loadedModules,
          "baseline settingsAppearance loadedModules",
        ),
        requiredModules: requireStringArray(
          contract.requiredModules,
          "baseline settingsAppearance requiredModules",
        ),
        forbiddenModules: requireStringArray(
          contract.forbiddenModules,
          "baseline settingsAppearance forbiddenModules",
        ),
      };
    })(),
    settingsDetailModules: requireStringArray(
      root.settingsDetailModules,
      "baseline settingsDetailModules",
    ),
    defaultEnglishStartup: (() => {
      const contract = requireObject(
        root.defaultEnglishStartup,
        "baseline defaultEnglishStartup",
      );
      return {
        loadedModules: requireStringArray(
          contract.loadedModules,
          "baseline defaultEnglishStartup loadedModules",
        ),
        forbiddenModulePrefixes: requireStringArray(
          contract.forbiddenModulePrefixes,
          "baseline defaultEnglishStartup forbiddenModulePrefixes",
        ),
      };
    })(),
    forbiddenEntryModules: requireStringArray(
      root.forbiddenEntryModules,
      "baseline forbiddenEntryModules",
    ),
    forbiddenModuleSubstrings: requireStringArray(
      root.forbiddenModuleSubstrings,
      "baseline forbiddenModuleSubstrings",
    ),
  };
}

/** List every source path the first-use baseline names, with how it is matched. */
function baselinePathEntries(baseline) {
  const staticContracts = [
    ["shell", baseline.shell],
    ["chatShell", baseline.chatShell],
    ["projectsShell", baseline.projectsShell],
    ["projectsToolbar", baseline.projectsToolbar],
    ["notesShell", baseline.notesShell],
    ["notesParagraphEditor", baseline.notesParagraphEditor],
    ["notesDatabaseTable", baseline.notesDatabaseTable],
    ["settingsAppearance", baseline.settingsAppearance],
  ];
  return [
    ...baselineEntries("routes module", baseline.routes.map((route) => route.module), "exact"),
    ...baseline.noVaultStartup.roots.flatMap((root) => (
      baselineEntries(`noVaultStartup ${root.name} loadedModules`, root.loadedModules, "exact")
    )),
    ...baselineEntries("noVaultStartup forbiddenModules", baseline.noVaultStartup.forbiddenModules, "exact"),
    ...baselineEntries(
      "noVaultStartup forbiddenModuleSubstrings",
      baseline.noVaultStartup.forbiddenModuleSubstrings,
      "pattern",
    ),
    ...staticContracts.flatMap(([name, contract]) => [
      ...baselineEntries(`${name} loadedModules`, contract.loadedModules, "exact"),
      ...baselineEntries(`${name} requiredModules`, contract.requiredModules ?? [], "exact"),
      ...baselineEntries(`${name} forbiddenModules`, contract.forbiddenModules, "exact"),
      ...baselineEntries(`${name} forbiddenModuleSubstrings`, contract.forbiddenModuleSubstrings ?? [], "pattern"),
    ]),
    ...baselineEntries("editorRuntime module", [baseline.editorRuntime.module], "exact"),
    ...baselineEntries(
      "editorRuntime requiredModuleSubstrings",
      baseline.editorRuntime.requiredModuleSubstrings,
      "pattern",
    ),
    ...baselineEntries("reviewRuntime module", [baseline.reviewRuntime.module], "exact"),
    ...baselineEntries(
      "reviewRuntime catalogModuleSubstrings",
      baseline.reviewRuntime.catalogModuleSubstrings,
      "pattern",
    ),
    ...baselineEntries("notesAdvancedModules", baseline.notesAdvancedModules, "exact"),
    ...baselineEntries("settingsDetailModules", baseline.settingsDetailModules, "exact"),
    ...baselineEntries("defaultEnglishStartup loadedModules", baseline.defaultEnglishStartup.loadedModules, "exact"),
    ...baselineEntries(
      "defaultEnglishStartup forbiddenModulePrefixes",
      baseline.defaultEnglishStartup.forbiddenModulePrefixes,
      "pattern",
    ),
    ...baselineEntries("forbiddenEntryModules", baseline.forbiddenEntryModules, "exact"),
    ...baselineEntries("forbiddenModuleSubstrings", baseline.forbiddenModuleSubstrings, "pattern"),
  ];
}

const baseline = readBaseline(await readJson(baselinePath));
assertBaselinePathsExist("first-use bundle", baselinePathEntries(baseline), clientFileSystem(clientDir));
const chunks = readChunks(await readJson(metadataPath));
const allModules = new Set(chunks.flatMap((chunk) => chunk.modules));
const entryModules = new Set(
  chunks.filter((chunk) => chunk.isEntry).flatMap((chunk) => chunk.modules),
);
const failures = [];
const chunksByFileName = new Map(chunks.map((chunk) => [chunk.fileName, chunk]));

function staticChunkClosure(rootChunks) {
  const visited = new Set();
  const visit = (chunk) => {
    if (!chunk || visited.has(chunk.fileName)) return;
    visited.add(chunk.fileName);
    for (const importedFile of chunk.imports) visit(chunksByFileName.get(importedFile));
  };
  for (const chunk of rootChunks) visit(chunk);
  return chunks.filter((chunk) => visited.has(chunk.fileName));
}
const routes = baseline.routes.map((route) => {
  const owner = chunks.find((chunk) => chunk.modules.includes(route.module));
  if (!owner) {
    failures.push(`${route.name} route module is absent: ${route.module}`);
    return { name: route.name, chunk: null, sourceModules: null };
  }
  const sourceModules = owner.modules.filter((moduleId) => moduleId.startsWith("src/")).length;
  if (sourceModules > route.maxSourceModules) {
    failures.push(
      `${route.name} route chunk has ${sourceModules} source modules, baseline allows ${route.maxSourceModules}`,
    );
  }
  return { name: route.name, chunk: owner.fileName, sourceModules };
});

function evaluateStaticModuleContract(contract, label) {
  const roots = contract.loadedModules.map((moduleId) => {
    const owner = chunks.find((chunk) => chunk.modules.includes(moduleId));
    if (!owner) failures.push(`${label} loaded module is absent: ${moduleId}`);
    return owner;
  }).filter(Boolean);
  const closure = staticChunkClosure(roots);
  const modules = new Set(closure.flatMap((chunk) => chunk.modules));
  for (const moduleId of contract.requiredModules ?? []) {
    if (!modules.has(moduleId)) failures.push(`${label} does not load required module: ${moduleId}`);
  }
  for (const moduleId of contract.forbiddenModules) {
    if (!allModules.has(moduleId)) {
      failures.push(`${label} forbidden module is absent from all chunks: ${moduleId}`);
    } else if (modules.has(moduleId)) {
      failures.push(`${label} loads forbidden module: ${moduleId}`);
    }
  }
  for (const substring of contract.forbiddenModuleSubstrings ?? []) {
    const matchingModules = [...allModules].filter((moduleId) => moduleId.includes(substring));
    if (matchingModules.length === 0) {
      failures.push(`${label} forbidden module substring matches no modules: ${substring}`);
      continue;
    }
    for (const moduleId of matchingModules) {
      if (modules.has(moduleId)) failures.push(`${label} loads forbidden module: ${moduleId}`);
    }
  }
  return { closure, modules };
}

const shellContract = evaluateStaticModuleContract(baseline.shell, "initial shell");

const noVaultStartupContracts = baseline.noVaultStartup.roots.map((root) => {
  const contract = evaluateStaticModuleContract({
    ...root,
    forbiddenModules: baseline.noVaultStartup.forbiddenModules,
    forbiddenModuleSubstrings: baseline.noVaultStartup.forbiddenModuleSubstrings,
  }, root.name);
  return { name: root.name, chunks: contract.closure.map((chunk) => chunk.fileName) };
});

const chatShellContract = evaluateStaticModuleContract(baseline.chatShell, "Chat shell");

const editorRuntimeChunk = chunks.find((chunk) => (
  chunk.modules.includes(baseline.editorRuntime.module)
));
if (!editorRuntimeChunk) {
  failures.push(`Editor runtime module is absent: ${baseline.editorRuntime.module}`);
} else {
  if (editorRuntimeChunk.isEntry) failures.push("Editor runtime is present in an entry chunk");
  const editorRuntimeClosure = staticChunkClosure([editorRuntimeChunk]);
  const editorRuntimeModules = new Set(editorRuntimeClosure.flatMap((chunk) => chunk.modules));
  for (const substring of baseline.editorRuntime.requiredModuleSubstrings) {
    if (![...editorRuntimeModules].some((moduleId) => moduleId.includes(substring))) {
      failures.push(`Editor runtime does not load required dependency: ${substring}`);
    }
  }
  for (const prefix of baseline.editorRuntime.forbiddenStaticChunkPrefixes) {
    const matchingChunk = editorRuntimeClosure.find((chunk) => chunk.fileName.startsWith(prefix));
    if (matchingChunk) {
      failures.push(`Editor runtime eagerly loads language chunk: ${matchingChunk.fileName}`);
    }
  }
  const editorDynamicImports = new Set(
    editorRuntimeClosure.flatMap((chunk) => chunk.dynamicImports),
  );
  for (const prefix of baseline.editorRuntime.requiredDynamicChunkPrefixes) {
    if (![...editorDynamicImports].some((fileName) => fileName.startsWith(prefix))) {
      failures.push(`Editor runtime has no on-demand chunk with prefix: ${prefix}`);
    }
  }
}

const reviewRuntimeChunk = chunks.find((chunk) => (
  chunk.modules.includes(baseline.reviewRuntime.module)
));
if (!reviewRuntimeChunk) {
  failures.push(`Review runtime module is absent: ${baseline.reviewRuntime.module}`);
} else {
  if (reviewRuntimeChunk.isEntry) {
    failures.push("Review runtime is present in an entry chunk");
  }
  const reviewRuntimeClosure = staticChunkClosure([reviewRuntimeChunk]);
  const reviewDynamicImports = new Set(
    reviewRuntimeClosure.flatMap((chunk) => chunk.dynamicImports),
  );
  for (const prefix of baseline.reviewRuntime.requiredDynamicChunkPrefixes) {
    if (![...reviewDynamicImports].some((fileName) => fileName.startsWith(prefix))) {
      failures.push(`Review runtime has no on-demand chunk with prefix: ${prefix}`);
    }
  }
}
for (const chunk of chunks) {
  const catalogModules = chunk.modules.filter((moduleId) => (
    baseline.reviewRuntime.catalogModuleSubstrings.some((substring) => (
      moduleId.includes(substring)
    ))
  ));
  if (catalogModules.length > baseline.reviewRuntime.maxCatalogModulesPerChunk) {
    failures.push(
      `Review catalog chunk ${chunk.fileName} contains ${catalogModules.length} catalog modules, baseline allows ${baseline.reviewRuntime.maxCatalogModulesPerChunk}`,
    );
  }
}

const projectsShellContract = evaluateStaticModuleContract(
  baseline.projectsShell,
  "Projects shell",
);
const projectsToolbarContract = evaluateStaticModuleContract(
  baseline.projectsToolbar,
  "Projects toolbar",
);
const notesShellContract = evaluateStaticModuleContract(
  baseline.notesShell,
  "Notes shell",
);
const notesParagraphEditorContract = evaluateStaticModuleContract(
  baseline.notesParagraphEditor,
  "Notes paragraph editor",
);
const notesDatabaseTableContract = evaluateStaticModuleContract(
  baseline.notesDatabaseTable,
  "Notes database table",
);
for (const moduleId of baseline.notesAdvancedModules) {
  const owner = chunks.find((chunk) => chunk.modules.includes(moduleId));
  if (!owner) failures.push(`Notes advanced module is absent from all chunks: ${moduleId}`);
  if (notesParagraphEditorContract.modules.has(moduleId)) {
    failures.push(`Notes paragraph editor eagerly loads advanced module: ${moduleId}`);
  }
}

const settingsAppearanceRoots = baseline.settingsAppearance.loadedModules.map((moduleId) => {
  const owner = chunks.find((chunk) => chunk.modules.includes(moduleId));
  if (!owner) failures.push(`Settings Appearance loaded module is absent: ${moduleId}`);
  return owner;
}).filter(Boolean);
const settingsAppearanceChunks = staticChunkClosure(settingsAppearanceRoots);
const settingsAppearanceModules = new Set(
  settingsAppearanceChunks.flatMap((chunk) => chunk.modules),
);
for (const moduleId of baseline.settingsAppearance.requiredModules) {
  if (!settingsAppearanceModules.has(moduleId)) {
    failures.push(`Settings Appearance does not load required module: ${moduleId}`);
  }
}
for (const moduleId of baseline.settingsAppearance.forbiddenModules) {
  if (!allModules.has(moduleId)) {
    failures.push(`Settings Appearance forbidden module is absent from all chunks: ${moduleId}`);
  } else if (settingsAppearanceModules.has(moduleId)) {
    failures.push(`module is loaded when opening Settings Appearance: ${moduleId}`);
  }
}

const settingsDetailChunks = baseline.settingsDetailModules.map((moduleId) => {
  const owner = chunks.find((chunk) => chunk.modules.includes(moduleId));
  if (!owner) failures.push(`Settings detail module is absent: ${moduleId}`);
  return { module: moduleId, chunk: owner?.fileName ?? null };
});
const settingsDetailChunkNames = settingsDetailChunks
  .map((entry) => entry.chunk)
  .filter((chunk) => chunk !== null);
if (new Set(settingsDetailChunkNames).size !== settingsDetailChunkNames.length) {
  failures.push("Settings detail modules are not emitted in distinct chunks");
}

const defaultEnglishStartupRoots = baseline.defaultEnglishStartup.loadedModules.map(
  (moduleId) => {
    const owner = chunks.find((chunk) => chunk.modules.includes(moduleId));
    if (!owner) failures.push(`default English startup module is absent: ${moduleId}`);
    return owner;
  },
).filter(Boolean);
const defaultEnglishStartupChunks = staticChunkClosure(defaultEnglishStartupRoots);
const defaultEnglishStartupModules = new Set(
  defaultEnglishStartupChunks.flatMap((chunk) => chunk.modules),
);
for (const prefix of baseline.defaultEnglishStartup.forbiddenModulePrefixes) {
  const matchingModules = [...allModules].filter((moduleId) => moduleId.startsWith(prefix));
  if (matchingModules.length === 0) {
    failures.push(`default English startup forbidden prefix matches no modules: ${prefix}`);
    continue;
  }
  for (const moduleId of matchingModules) {
    if (defaultEnglishStartupModules.has(moduleId)) {
      failures.push(`module is loaded during default English startup: ${moduleId}`);
    }
  }
}

for (const moduleId of baseline.forbiddenEntryModules) {
  if (!allModules.has(moduleId)) {
    failures.push(`forbidden entry module is absent from all chunks: ${moduleId}`);
  } else if (entryModules.has(moduleId)) {
    failures.push(`forbidden module is present in an entry chunk: ${moduleId}`);
  }
}

for (const substring of baseline.forbiddenModuleSubstrings) {
  for (const moduleId of allModules) {
    if (moduleId.includes(substring)) {
      failures.push(`forbidden dependency module is present: ${moduleId}`);
    }
  }
}

if (failures.length > 0) {
  throw new Error(`first-use bundle contract failed:\n${failures.map((failure) => `* ${failure}`).join("\n")}`);
}

console.log(JSON.stringify({
  routes,
  noVaultStartup: noVaultStartupContracts,
  shell: {
    chunks: shellContract.closure.map((chunk) => chunk.fileName),
    requiredModules: baseline.shell.requiredModules,
    forbiddenModules: baseline.shell.forbiddenModules,
  },
  chatShell: {
    chunks: chatShellContract.closure.map((chunk) => chunk.fileName),
    sourceModules: [...chatShellContract.modules]
      .filter((moduleId) => moduleId.startsWith("src/")).length,
    requiredModules: baseline.chatShell.requiredModules,
    forbiddenModules: baseline.chatShell.forbiddenModules,
    forbiddenModuleSubstrings: baseline.chatShell.forbiddenModuleSubstrings,
  },
  reviewRuntime: reviewRuntimeChunk ? {
    chunk: reviewRuntimeChunk.fileName,
    dynamicChunks: reviewRuntimeChunk.dynamicImports.length,
    maxCatalogModulesPerChunk: baseline.reviewRuntime.maxCatalogModulesPerChunk,
  } : null,
  editorRuntime: editorRuntimeChunk ? {
    chunk: editorRuntimeChunk.fileName,
    dynamicChunks: editorRuntimeChunk.dynamicImports.length,
    forbiddenStaticChunkPrefixes: baseline.editorRuntime.forbiddenStaticChunkPrefixes,
  } : null,
  projectsShell: {
    chunks: projectsShellContract.closure.map((chunk) => chunk.fileName),
    sourceModules: [...projectsShellContract.modules]
      .filter((moduleId) => moduleId.startsWith("src/")).length,
    forbiddenModules: baseline.projectsShell.forbiddenModules,
  },
  projectsToolbar: {
    chunks: projectsToolbarContract.closure.map((chunk) => chunk.fileName),
    sourceModules: [...projectsToolbarContract.modules]
      .filter((moduleId) => moduleId.startsWith("src/")).length,
    forbiddenModules: baseline.projectsToolbar.forbiddenModules,
  },
  notesShell: {
    chunks: notesShellContract.closure.map((chunk) => chunk.fileName),
    sourceModules: [...notesShellContract.modules]
      .filter((moduleId) => moduleId.startsWith("src/")).length,
    forbiddenModules: baseline.notesShell.forbiddenModules,
  },
  notesParagraphEditor: {
    chunks: notesParagraphEditorContract.closure.map((chunk) => chunk.fileName),
    sourceModules: [...notesParagraphEditorContract.modules]
      .filter((moduleId) => moduleId.startsWith("src/")).length,
    forbiddenModules: baseline.notesParagraphEditor.forbiddenModules,
  },
  notesAdvancedModules: baseline.notesAdvancedModules,
  notesDatabaseTable: {
    chunks: notesDatabaseTableContract.closure.map((chunk) => chunk.fileName),
    sourceModules: [...notesDatabaseTableContract.modules]
      .filter((moduleId) => moduleId.startsWith("src/")).length,
    forbiddenModules: baseline.notesDatabaseTable.forbiddenModules,
  },
  settingsAppearance: {
    chunks: settingsAppearanceChunks.map((chunk) => chunk.fileName),
    sourceModules: [...settingsAppearanceModules]
      .filter((moduleId) => moduleId.startsWith("src/")).length,
    requiredModules: baseline.settingsAppearance.requiredModules,
    forbiddenModules: baseline.settingsAppearance.forbiddenModules,
  },
  settingsDetailChunks,
  defaultEnglishStartup: {
    chunks: defaultEnglishStartupChunks.map((chunk) => chunk.fileName),
    sourceModules: [...defaultEnglishStartupModules]
      .filter((moduleId) => moduleId.startsWith("src/")).length,
    forbiddenModulePrefixes: baseline.defaultEnglishStartup.forbiddenModulePrefixes,
  },
  forbiddenEntryModules: baseline.forbiddenEntryModules,
  forbiddenModuleSubstrings: baseline.forbiddenModuleSubstrings,
}, null, 2));
