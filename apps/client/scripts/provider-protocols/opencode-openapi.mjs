import { createHash } from "node:crypto";
import { readFile, writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const artifactUrl = new URL(
  "../../../../crates/ganbaru-chat-providers/src/opencode/compat/openapi-1.14.19.json",
  import.meta.url,
);
const requiredOperations = [
  "/agent get",
  "/command get",
  "/formatter get",
  "/lsp get",
  "/mcp get",
  "/mcp post",
  "/permission get",
  "/permission/{requestID}/reply post",
  "/question get",
  "/question/{requestID}/reject post",
  "/question/{requestID}/reply post",
  "/session post",
  "/session/status get",
  "/session/{sessionID} delete",
  "/session/{sessionID}/fork post",
  "/session/{sessionID}/abort post",
  "/session/{sessionID}/command post",
  "/session/{sessionID}/message get",
  "/session/{sessionID}/prompt_async post",
  "/session/{sessionID}/revert post",
  "/session/{sessionID}/unrevert post",
];

/** Read a named command-line flag without interpreting its value as syntax. */
function flag(name) {
  const index = process.argv.indexOf(name);
  return index >= 0 ? process.argv[index + 1] ?? null : null;
}

/** Load an OpenAPI document from a local path or explicit HTTP endpoint. */
async function readSource(source) {
  if (/^https?:\/\//u.test(source)) {
    const response = await fetch(source, { redirect: "error" });
    if (!response.ok) throw new Error(`OpenCode schema request failed with HTTP ${response.status}.`);
    return Buffer.from(await response.arrayBuffer());
  }
  return readFile(source);
}

/** Convert the complete OpenAPI path map into a deterministic operation surface. */
function operations(document) {
  if (!document || typeof document !== "object" || Array.isArray(document)) {
    throw new Error("OpenCode OpenAPI document must be an object.");
  }
  if (!document.paths || typeof document.paths !== "object" || Array.isArray(document.paths)) {
    throw new Error("OpenCode OpenAPI paths are missing.");
  }
  const methods = new Set(["delete", "get", "head", "options", "patch", "post", "put", "trace"]);
  return Object.entries(document.paths)
    .flatMap(([path, entry]) => {
      if (!entry || typeof entry !== "object" || Array.isArray(entry)) return [];
      return Object.keys(entry)
        .filter((method) => methods.has(method))
        .map((method) => `${path} ${method}`);
    })
    .sort();
}

/** Validate the committed compatibility summary used by runtime fixture tests. */
function validateArtifact(artifact) {
  if (artifact.openCodeVersion !== "1.14.19") throw new Error("Unexpected minimum OpenCode version.");
  if (!/^[0-9a-f]{64}$/u.test(artifact.documentSha256)) throw new Error("Invalid OpenCode schema digest.");
  if (!Number.isSafeInteger(artifact.documentByteSize) || artifact.documentByteSize <= 0) {
    throw new Error("Invalid OpenCode schema byte size.");
  }
  if (!Array.isArray(artifact.operations)) throw new Error("OpenCode operation snapshot is missing.");
  for (const operation of requiredOperations) {
    if (!artifact.operations.includes(operation)) throw new Error(`Required OpenCode operation is missing: ${operation}`);
  }
}

// Checks the committed artifact by default; --source compares it with a schema and
// --write regenerates it from that schema.
const artifact = JSON.parse(await readFile(artifactUrl, "utf8"));
validateArtifact(artifact);
const source = flag("--source");
const writeArtifact = process.argv.includes("--write");

if (writeArtifact && !source) {
  throw new Error("--write requires --source <path-or-url> pointing at the OpenCode OpenAPI document.");
}

if (!source) {
  console.log(`OpenCode ${artifact.openCodeVersion} OpenAPI compatibility artifact is valid.`);
  process.exit(0);
}

const bytes = await readSource(source);
const document = JSON.parse(bytes.toString("utf8"));
const generated = {
  openCodeVersion: flag("--version") ?? artifact.openCodeVersion,
  sourceTag: flag("--tag") ?? artifact.sourceTag,
  sourceCommit: flag("--commit") ?? artifact.sourceCommit,
  openApiVersion: document.openapi,
  documentSha256: createHash("sha256").update(bytes).digest("hex"),
  documentByteSize: bytes.length,
  operations: operations(document),
};
validateArtifact(generated);

if (writeArtifact) {
  await writeFile(artifactUrl, `${JSON.stringify(generated, null, 2)}\n`, "utf8");
  console.log(`Updated ${fileURLToPath(artifactUrl)}.`);
} else if (JSON.stringify(generated) !== JSON.stringify(artifact)) {
  throw new Error("OpenCode OpenAPI compatibility artifact is stale. Run the update command with --write.");
} else {
  console.log(`OpenCode ${artifact.openCodeVersion} OpenAPI schema matches the committed artifact.`);
}
