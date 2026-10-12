import { createHash } from "node:crypto";
import { execFileSync, spawnSync } from "node:child_process";
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const scriptDirectory = dirname(fileURLToPath(import.meta.url));
const clientDirectory = resolve(scriptDirectory, "..", "..");
const outputDirectory = join(
  clientDirectory,
  "../../crates/ganbaru-chat-providers/src/codex/compat",
);
const manifestPath = join(outputDirectory, "manifest.json");
const schemaNames = [
  "codex_app_server_protocol.schemas.json",
  "codex_app_server_protocol.v2.schemas.json",
];
// Checks committed artifacts by default; --check-installed compares them with the
// installed Codex CLI and --write regenerates them from it.
const argumentsSet = new Set(process.argv.slice(2));
const supportedArguments = new Set(["--check-installed", "--write"]);
for (const argument of argumentsSet) {
  if (!supportedArguments.has(argument)) {
    throw new Error(`Unsupported argument: ${argument}`);
  }
}
if (argumentsSet.size > 1) {
  throw new Error("Choose either --check-installed or --write");
}
const writeArtifacts = argumentsSet.has("--write");
const checkInstalled = argumentsSet.has("--check-installed");
const checkOnly = !writeArtifacts && !checkInstalled;

function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

function canonicalize(value) {
  if (Array.isArray(value)) return value.map(canonicalize);
  if (value === null || typeof value !== "object") return value;
  return Object.fromEntries(
    Object.keys(value)
      .sort()
      .map((key) => [key, canonicalize(value[key])]),
  );
}

function codexVersion() {
  const result = spawnSync("codex", ["--version"], { encoding: "utf8" });
  if (result.error?.code === "ENOENT") return null;
  if (result.status !== 0) throw new Error(result.stderr || "Codex version probe failed");
  const version = result.stdout.trim();
  if (!version || version.length > 200) throw new Error("Codex returned an invalid version");
  return version;
}

function verifyCommittedArtifacts() {
  const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
  if (manifest.schemaVersion !== 1 || typeof manifest.cliVersion !== "string") {
    throw new Error("Codex compatibility manifest is invalid");
  }
  for (const name of schemaNames) {
    const bytes = readFileSync(join(outputDirectory, name));
    const expected = manifest.files?.[name];
    if (expected?.sha256 !== sha256(bytes) || expected?.bytes !== bytes.length) {
      throw new Error(`Committed Codex schema checksum is invalid: ${name}`);
    }
  }
  return manifest;
}

function generateSchemas() {
  const directory = mkdtempSync(join(tmpdir(), "ganbaru-codex-schema-"));
  try {
    execFileSync("codex", ["app-server", "generate-json-schema", "--out", directory], {
      stdio: "pipe",
    });
    return schemaNames.map((name) => {
      const schema = JSON.parse(readFileSync(join(directory, name), "utf8"));
      return {
        name,
        bytes: Buffer.from(`${JSON.stringify(canonicalize(schema), null, 2)}\n`),
      };
    });
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
}

if (checkOnly) {
  const manifest = verifyCommittedArtifacts();
  process.stdout.write(
    `Committed Codex app-server schema checksums are valid (${manifest.cliVersion}).\n`,
  );
} else if (checkInstalled) {
  const manifest = verifyCommittedArtifacts();
  const installedVersion = codexVersion();
  if (installedVersion === null) {
    throw new Error("Codex CLI is required to compare installed schemas");
  }
  const driftedSchemas = [];
  for (const generated of generateSchemas()) {
    const committed = readFileSync(join(outputDirectory, generated.name));
    if (!committed.equals(generated.bytes)) {
      driftedSchemas.push(generated.name);
    }
  }
  if (driftedSchemas.length > 0) {
    throw new Error(
      `Codex app-server schema drift between ${manifest.cliVersion} and ${installedVersion}: ${driftedSchemas.join(", ")}`,
    );
  }
  process.stdout.write(`Installed Codex app-server schemas match ${manifest.cliVersion}.\n`);
} else {
  const installedVersion = codexVersion();
  if (installedVersion === null) throw new Error("Codex CLI is required to update schemas");
  const generated = generateSchemas();
  mkdirSync(outputDirectory, { recursive: true });
  const files = {};
  for (const artifact of generated) {
    const destination = join(outputDirectory, artifact.name);
    writeFileSync(destination, artifact.bytes);
    files[artifact.name] = {
      sha256: sha256(artifact.bytes),
      bytes: artifact.bytes.length,
    };
  }
  writeFileSync(
    manifestPath,
    `${JSON.stringify({ schemaVersion: 1, cliVersion: installedVersion, files }, null, 2)}\n`,
  );
  process.stdout.write(`Updated Codex app-server schemas for ${installedVersion}.\n`);
}
